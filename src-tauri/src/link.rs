//! Deskmate Link v1: authenticated WebSocket handshake and encrypted JSON frames.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::config::AppConfig;
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;
const WS_PATH: &str = "/api/deskmate_link/ws";
const MAX_SKEW_SECS: i64 = 90;

pub fn normalize_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let mut url = url::Url::parse(raw).map_err(|e| format!("invalid Link URL: {e}"))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err("Link URL must use ws:// or wss://".into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err("Link URL cannot contain credentials, query or fragment".into());
    }
    match url.path() {
        "" | "/" => url.set_path(WS_PATH),
        WS_PATH => {}
        _ => return Err(format!("Link URL path must be {WS_PATH}")),
    }
    Ok(url.to_string())
}

pub fn validate_pairing_key(raw: &str) -> Result<[u8; 32], String> {
    let decoded = B64.decode(raw.trim()).map_err(|_| "Link pairing key must be base64".to_string())?;
    decoded.try_into().map_err(|_| "Link pairing key must decode to exactly 32 bytes".into())
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn mac(psk: &[u8; 32], message: &str) -> [u8; 32] {
    let mut hmac = <HmacSha256 as Mac>::new_from_slice(psk).expect("fixed-size HMAC key");
    hmac.update(message.as_bytes());
    hmac.finalize().into_bytes().into()
}

fn verify_mac(psk: &[u8; 32], message: &str, encoded: &str) -> Result<(), String> {
    let supplied = B64.decode(encoded).map_err(|_| "invalid handshake MAC".to_string())?;
    let mut hmac = <HmacSha256 as Mac>::new_from_slice(psk).expect("fixed-size HMAC key");
    hmac.update(message.as_bytes());
    hmac.verify_slice(&supplied).map_err(|_| "handshake authentication failed".into())
}

fn derive_keys(psk: &[u8; 32], cn: &[u8; 16], sn: &[u8; 16]) -> Result<([u8; 32], [u8; 32]), String> {
    let mut salt = [0u8; 32];
    salt[..16].copy_from_slice(cn);
    salt[16..].copy_from_slice(sn);
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), psk);
    let mut c2s = [0u8; 32];
    let mut s2c = [0u8; 32];
    hkdf.expand(b"dml1 c2s", &mut c2s).map_err(|_| "HKDF c2s failed".to_string())?;
    hkdf.expand(b"dml1 s2c", &mut s2c).map_err(|_| "HKDF s2c failed".to_string())?;
    Ok((c2s, s2c))
}

struct FrameCodec {
    cipher: Aes256Gcm,
    nonce_prefix: [u8; 4],
    aad: Vec<u8>,
    counter: u64,
}

impl FrameCodec {
    fn new(key: &[u8; 32], node: &str, direction: &str) -> Self {
        let prefix = if direction == "c2s" { [1, 0, 0, 0] } else { [2, 0, 0, 0] };
        Self {
            cipher: Aes256Gcm::new_from_slice(key).expect("AES-256 key"),
            nonce_prefix: prefix,
            aad: format!("{node}|{direction}").into_bytes(),
            counter: 0,
        }
    }

    fn nonce(&self, counter: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.nonce_prefix);
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        nonce
    }

    fn encrypt(&mut self, value: &Value) -> Result<String, String> {
        self.counter = self.counter.checked_add(1).ok_or_else(|| "frame counter exhausted".to_string())?;
        let plaintext = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        let nonce = self.nonce(self.counter);
        let ciphertext = self.cipher.encrypt(
            Nonce::from_slice(&nonce),
            Payload { msg: &plaintext, aad: &self.aad },
        ).map_err(|_| "frame encryption failed".to_string())?;
        Ok(json!({"t": "e", "n": self.counter, "p": B64.encode(ciphertext)}).to_string())
    }

    fn decrypt(&mut self, frame: &str) -> Result<Value, String> {
        let frame: Value = serde_json::from_str(frame).map_err(|_| "invalid encrypted frame JSON".to_string())?;
        if frame.get("t").and_then(Value::as_str) != Some("e") {
            return Err("unencrypted post-handshake frame".into());
        }
        let counter = frame.get("n").and_then(Value::as_u64).ok_or_else(|| "missing frame counter".to_string())?;
        if counter <= self.counter {
            return Err("replayed or out-of-order frame".into());
        }
        let ciphertext = B64.decode(frame.get("p").and_then(Value::as_str).ok_or_else(|| "missing frame payload".to_string())?)
            .map_err(|_| "invalid frame payload".to_string())?;
        let nonce = self.nonce(counter);
        let plaintext = self.cipher.decrypt(
            Nonce::from_slice(&nonce),
            Payload { msg: &ciphertext, aad: &self.aad },
        ).map_err(|_| "frame authentication failed".to_string())?;
        let value = serde_json::from_slice(&plaintext).map_err(|_| "invalid encrypted payload JSON".to_string())?;
        self.counter = counter;
        Ok(value)
    }
}

pub async fn restart(app: AppHandle) {
    let state = app.state::<AppState>();
    if let Some(tx) = state.stop_tx.lock().await.take() {
        let _ = tx.send(true);
    }
    if let Some(client) = state.client.lock().await.take() {
        let _ = client.disconnect().await;
    }
    *state.link_tx.lock().await = None;

    let cfg = state.config.lock().await.clone();
    if !cfg.configured || cfg.link_url.is_empty() {
        crate::mqtt::set_status(&app, false, "Not configured");
        return;
    }
    let Some(raw_key) = crate::config::get_link_key(&cfg.node_id) else {
        crate::mqtt::set_status(&app, false, "Link pairing key missing");
        return;
    };
    let psk = match validate_pairing_key(&raw_key) {
        Ok(key) => key,
        Err(error) => {
            crate::mqtt::set_status(&app, false, &error);
            return;
        }
    };
    let (stop_tx, stop_rx) = watch::channel(false);
    *state.stop_tx.lock().await = Some(stop_tx);
    crate::mqtt::set_status(&app, false, "Connecting Link...");

    let app_connection = app.clone();
    let cfg_connection = cfg.clone();
    let mut stop_connection = stop_rx.clone();
    tauri::async_runtime::spawn(async move {
        let mut endpoints = vec![cfg_connection.link_url.clone()];
        if !cfg_connection.link_url_remote.is_empty() && cfg_connection.link_url_remote != cfg_connection.link_url {
            endpoints.push(cfg_connection.link_url_remote.clone());
        }
        let mut endpoint_index = 0usize;
        let mut failures = 0u32;
        loop {
            if *stop_connection.borrow() { break; }
            let label = if endpoints.len() == 1 { "" } else if endpoint_index == 0 { " (local)" } else { " (remote)" };
            crate::mqtt::set_status(&app_connection, false, &format!("Connecting Link{label}..."));
            let session_result = run_session(&app_connection, &cfg_connection, &psk, &endpoints[endpoint_index], stop_connection.clone()).await;
            *app_connection.state::<AppState>().link_tx.lock().await = None;
            let mut retry_delay = if endpoints.len() > 1 { 2 } else { 5 };
            match session_result {
                Ok(()) if *stop_connection.borrow() => break,
                Ok(()) => failures = 0,
                Err(error) => {
                    failures += 1;
                    crate::mqtt::set_status(&app_connection, false, &format!("Link error{label}: {error}"));
                    if endpoints.len() > 1 && failures >= 2 {
                        endpoint_index = (endpoint_index + 1) % endpoints.len();
                        failures = 0;
                        retry_delay = 3;
                    }
                }
            }
            tokio::select! {
                _ = stop_connection.changed() => break,
                _ = tokio::time::sleep(Duration::from_secs(retry_delay)) => {}
            }
        }
        *app_connection.state::<AppState>().link_tx.lock().await = None;
    });

    let app_sensors = app.clone();
    let mut stop_sensors = stop_rx;
    tauri::async_runtime::spawn(async move {
        let mut collector: Option<crate::sensors::Collector> = None;
        loop {
            let secs = {
                let state = app_sensors.state::<AppState>();
                let cfg = state.config.lock().await.clone();
                let connected = state.status.lock().await.connected;
                let secs = cfg.publish_interval_secs.clamp(2, 3600);
                let mut current = collector.take().unwrap_or_else(crate::sensors::Collector::new);
                let (returned, values) = tokio::task::spawn_blocking(move || {
                    let values = current.collect(&cfg, connected);
                    (current, values)
                }).await.unwrap_or_else(|_| (crate::sensors::Collector::new(), HashMap::new()));
                collector = Some(returned);
                crate::transport::publish_states(&app_sensors, &values).await;
                secs
            };
            tokio::select! {
                _ = stop_sensors.changed() => break,
                _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
            }
        }
    });
}

async fn run_session(
    app: &AppHandle,
    cfg: &AppConfig,
    psk: &[u8; 32],
    endpoint: &str,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let (mut socket, _) = tokio_tungstenite::connect_async(endpoint).await.map_err(|e| e.to_string())?;
    let mut cn = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut cn);
    let cn_b64 = B64.encode(cn);
    let ts = now_unix();
    let hello_text = format!("hello|{}|{}|{}", cfg.node_id, cn_b64, ts);
    let hello = json!({"t": "hello", "v": 1, "node": cfg.node_id, "cn": cn_b64, "ts": ts, "mac": B64.encode(mac(psk, &hello_text))});
    socket.send(Message::Text(hello.to_string().into())).await.map_err(|e| e.to_string())?;
    let welcome_message = tokio::time::timeout(Duration::from_secs(10), socket.next()).await
        .map_err(|_| "welcome timeout".to_string())?
        .ok_or_else(|| "connection closed before welcome".to_string())?
        .map_err(|e| e.to_string())?;
    let welcome_text = welcome_message.into_text().map_err(|_| "welcome must be a text frame".to_string())?;
    let welcome: Value = serde_json::from_str(welcome_text.as_str()).map_err(|_| "invalid welcome JSON".to_string())?;
    if welcome.get("t").and_then(Value::as_str) != Some("welcome") {
        return Err("expected welcome".into());
    }
    let sn_b64 = welcome.get("sn").and_then(Value::as_str).ok_or_else(|| "welcome missing sn".to_string())?;
    let sn_vec = B64.decode(sn_b64).map_err(|_| "invalid server nonce".to_string())?;
    let sn: [u8; 16] = sn_vec.try_into().map_err(|_| "server nonce must be 16 bytes".to_string())?;
    let server_ts = welcome.get("ts").and_then(Value::as_i64).ok_or_else(|| "welcome missing ts".to_string())?;
    if (now_unix() - server_ts).abs() > MAX_SKEW_SECS {
        return Err("welcome timestamp outside allowed skew".into());
    }
    let signed = format!("welcome|{}|{}|{}|{}", cfg.node_id, cn_b64, sn_b64, server_ts);
    verify_mac(psk, &signed, welcome.get("mac").and_then(Value::as_str).unwrap_or(""))?;
    let (c2s, s2c) = derive_keys(psk, &cn, &sn)?;
    let mut encoder = FrameCodec::new(&c2s, &cfg.node_id, "c2s");
    let mut decoder = FrameCodec::new(&s2c, &cfg.node_id, "s2c");
    let (mut writer, mut reader) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
    *app.state::<AppState>().link_tx.lock().await = Some(out_tx.clone());
    out_tx.send(crate::discovery::build_link_declare(cfg)).map_err(|e| e.to_string())?;
    let cached = app.state::<AppState>().sensor_values.lock().await.clone();
    if !cached.is_empty() {
        out_tx.send(json!({"t": "state", "s": typed_states(&cached)})).map_err(|e| e.to_string())?;
    }
    crate::mqtt::set_status(app, true, "Connected (Link)");
    log::info!("Deskmate Link connected");
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = ping.tick() => { let _ = out_tx.send(json!({"t": "ping"})); }
            outbound = out_rx.recv() => {
                let Some(payload) = outbound else { break };
                let frame = encoder.encrypt(&payload)?;
                writer.send(Message::Text(frame.into())).await.map_err(|e| e.to_string())?;
            }
            inbound = reader.next() => {
                let message = inbound.ok_or_else(|| "Link websocket closed".to_string())?.map_err(|e| e.to_string())?;
                match message {
                    Message::Text(text) => {
                        let payload = decoder.decrypt(text.as_str())?;
                        handle_incoming(app, &out_tx, payload).await;
                    }
                    Message::Close(_) => break,
                    Message::Ping(data) => writer.send(Message::Pong(data)).await.map_err(|e| e.to_string())?,
                    _ => {}
                }
            }
        }
    }
    let stopped = *stop.borrow();
    *app.state::<AppState>().link_tx.lock().await = None;
    let _ = writer.close().await;
    if stopped { Ok(()) } else { Err("Link websocket closed".into()) }
}

async fn handle_incoming(app: &AppHandle, tx: &mpsc::UnboundedSender<Value>, payload: Value) {
    match payload.get("t").and_then(Value::as_str) {
        Some("cmd") => {
            let id = payload.get("id").cloned().unwrap_or(Value::Null);
            let key = payload.get("key").and_then(Value::as_str).unwrap_or("");
            let action = payload.get("action").and_then(Value::as_str).unwrap_or("set");
            let result = crate::transport::handle_command(app, key, action, payload.get("value")).await;
            let mut ack = json!({"t": "ack", "id": id, "ok": result.is_ok()});
            if let Err(error) = result { ack["error"] = json!(error); }
            let _ = tx.send(ack);
        }
        Some("notify") => {
            let id = payload.get("id").cloned().unwrap_or(Value::Null);
            let actions: Vec<Value> = payload.get("actions").and_then(Value::as_array).map(|items| {
                items.iter().map(|item| json!({
                    "title": item.get("title").and_then(Value::as_str).unwrap_or(""),
                    "action": item.get("id").and_then(Value::as_str).unwrap_or(""),
                })).collect()
            }).unwrap_or_default();
            let notification = json!({
                "title": payload.get("title").and_then(Value::as_str).unwrap_or(""),
                "message": payload.get("message").and_then(Value::as_str).unwrap_or(""),
                "image": payload.get("image").cloned().unwrap_or(Value::Null),
                "actions": actions,
            });
            crate::transport::handle_notify(app, &notification.to_string()).await;
            let _ = tx.send(json!({"t": "ack", "id": id, "ok": true}));
        }
        Some("ping") => { let _ = tx.send(json!({"t": "pong"})); }
        Some("pong") => {}
        _ => log::warn!("unknown Deskmate Link payload type"),
    }
}

fn typed_states(values: &HashMap<String, String>) -> Value {
    let mut states = Map::new();
    for (key, value) in values {
        let component = crate::sensors::SENSOR_DEFS.iter().find(|definition| definition.id == key).map(|definition| definition.component);
        let typed = match component {
            Some("binary_sensor") => Value::Bool(value.eq_ignore_ascii_case("ON") || value.eq_ignore_ascii_case("true") || value == "1"),
            Some("number") => value.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or_else(|| Value::String(value.clone())),
            _ if key == "keep_awake" => Value::Bool(value.eq_ignore_ascii_case("ON")),
            _ => Value::String(value.clone()),
        };
        states.insert(key.clone(), typed);
    }
    Value::Object(states)
}

pub async fn send(app: &AppHandle, payload: Value) -> bool {
    app.state::<AppState>().link_tx.lock().await.as_ref().map(|tx| tx.send(payload).is_ok()).unwrap_or(false)
}

pub async fn publish_states_network(app: &AppHandle, values: &HashMap<String, String>) -> usize {
    if send(app, json!({"t": "state", "s": typed_states(values)})).await { values.len() } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_link_url() {
        assert_eq!(normalize_url("ws://ha.local:8123").unwrap(), "ws://ha.local:8123/api/deskmate_link/ws");
        assert!(normalize_url("https://ha.local").is_err());
        assert!(normalize_url("wss://user@ha.local").is_err());
    }

    #[test]
    fn matches_home_assistant_python_vectors() {
        let vector: Value = serde_json::from_str(include_str!("../tests/fixtures/deskmate_link_v1.json")).unwrap();
        let psk = validate_pairing_key(vector["psk_b64"].as_str().unwrap()).unwrap();
        let node = vector["node"].as_str().unwrap();
        let cn_b64 = vector["cn_b64"].as_str().unwrap();
        let sn_b64 = vector["sn_b64"].as_str().unwrap();
        let hello_ts = vector["hello_ts"].as_i64().unwrap();
        let welcome_ts = vector["welcome_ts"].as_i64().unwrap();
        assert_eq!(
            B64.encode(mac(&psk, &format!("hello|{node}|{cn_b64}|{hello_ts}"))),
            vector["hello_mac_b64"].as_str().unwrap()
        );
        assert_eq!(
            B64.encode(mac(&psk, &format!("welcome|{node}|{cn_b64}|{sn_b64}|{welcome_ts}"))),
            vector["welcome_mac_b64"].as_str().unwrap()
        );
        let cn: [u8; 16] = B64.decode(cn_b64).unwrap().try_into().unwrap();
        let sn: [u8; 16] = B64.decode(sn_b64).unwrap().try_into().unwrap();
        let (c2s, s2c) = derive_keys(&psk, &cn, &sn).unwrap();
        assert_eq!(B64.encode(c2s), vector["c2s_key_b64"].as_str().unwrap());
        assert_eq!(B64.encode(s2c), vector["s2c_key_b64"].as_str().unwrap());
        let mut decoder = FrameCodec::new(&c2s, node, "c2s");
        let frame = vector["python_c2s_frame"].to_string();
        assert_eq!(decoder.decrypt(&frame).unwrap(), vector["payload"]);
        assert!(decoder.decrypt(&frame).is_err(), "the same counter must be rejected as replay");
    }
}
