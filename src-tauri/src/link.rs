//! Deskmate Link v2: authenticated WebSocket handshake and encrypted JSON frames.
//!
//! The pairing key (PSK) authenticates both ends; session keys come from an
//! ephemeral X25519 exchange mixed with the PSK, so a key leaked later does not
//! decrypt recorded sessions. Every handshake field is covered by the MAC
//! through a length-prefixed encoding. Protocol description: docs/LINK.md.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chacha20poly1305::ChaCha20Poly1305;
use futures_util::{SinkExt, StreamExt};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

use crate::config::AppConfig;
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;
/// A 32-byte secret that is wiped from memory when dropped.
pub type SecretKey = Zeroizing<[u8; 32]>;

const WS_PATH: &str = "/api/deskmate_link/ws";
const MAX_SKEW_SECS: i64 = 90;
const PROTOCOL_VERSION: u64 = 2;

/// Why a Link session ended. Drives both the status text and the retry delay:
/// a rejected pairing is not worth retrying every two seconds, and repeating it
/// only pushes the client into the server-side lockout.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkFailure {
    /// The pairing key, the node, the cascade setting or the protocol version
    /// does not match. Retrying will not help until someone changes settings.
    Auth,
    /// Home Assistant temporarily refused this node after repeated failures.
    Locked,
    /// This computer's clock and Home Assistant's differ by more than 90 s.
    Clock,
    /// Network, URL or a dropped connection.
    Transport,
}

struct LinkError {
    kind: LinkFailure,
    message: String,
}

impl LinkError {
    fn new(kind: LinkFailure, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<String> for LinkError {
    fn from(message: String) -> Self {
        Self::new(LinkFailure::Transport, message)
    }
}

impl From<&str> for LinkError {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

const LOCKED_MESSAGE: &str =
    "Home Assistant is temporarily refusing this node after repeated failed handshakes";
const CLOCK_MESSAGE: &str =
    "this computer's clock differs from Home Assistant's by more than 90 seconds - sync the Windows time (Settings > Time & language > Sync now)";

/// A 429 during the WebSocket upgrade is the server-side handshake lockout,
/// not an ordinary network problem.
fn classify_connect_error(error: tokio_tungstenite::tungstenite::Error) -> LinkError {
    if let tokio_tungstenite::tungstenite::Error::Http(response) = &error {
        if response.status().as_u16() == 429 {
            return LinkError::new(LinkFailure::Locked, LOCKED_MESSAGE);
        }
    }
    LinkError::from(error.to_string())
}

/// Maps the server's `reject` reason to a failure the user can act on.
fn classify_reject(reason: &str) -> LinkError {
    match reason {
        "locked" => LinkError::new(LinkFailure::Locked, LOCKED_MESSAGE),
        "clock" => LinkError::new(LinkFailure::Clock, CLOCK_MESSAGE),
        "cascade" => LinkError::new(
            LinkFailure::Auth,
            "cascade encryption is on at one end only - enable or disable it on both (Geeky stuff here, Reconfigure in Home Assistant)",
        ),
        "version" => LinkError::new(
            LinkFailure::Auth,
            "Home Assistant does not accept this protocol version - update the Deskmate Link integration to 0.6.0 or newer",
        ),
        _ => LinkError::new(
            LinkFailure::Auth,
            "Home Assistant rejected the pairing - the pairing key does not match, or the integration is older than 0.6.0 and needs an update",
        ),
    }
}

/// Retry delay after a non-network failure, in seconds.
fn auth_retry_delay(kind: LinkFailure, attempts: u32) -> u64 {
    match kind {
        LinkFailure::Locked => 60,
        LinkFailure::Clock => 30,
        _ => match attempts {
            0 | 1 => 5,
            2 => 15,
            3 => 30,
            _ => 60,
        },
    }
}

pub fn normalize_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let mut url = url::Url::parse(raw).map_err(|e| format!("invalid Link URL: {e}"))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err("Link URL must use ws:// or wss://".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Link URL cannot contain credentials, query or fragment".into());
    }
    match url.path() {
        "" | "/" => url.set_path(WS_PATH),
        WS_PATH => {}
        _ => return Err(format!("Link URL path must be {WS_PATH}")),
    }
    if url.scheme() == "ws" && !is_private_host(&url) {
        return Err(
            "plain ws:// is only allowed for LAN, .local and Tailscale addresses - use wss:// for anything reachable from the internet"
                .into(),
        );
    }
    Ok(url.to_string())
}

/// Hosts where an unencrypted WebSocket stays inside a trusted or already
/// encrypted network: RFC 1918, loopback, link-local, Tailscale's CGNAT range
/// and IPv6 ULA, plus mDNS and MagicDNS names. Frames are end-to-end encrypted
/// either way; this keeps the node name and traffic pattern off the internet.
fn is_private_host(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            let [a, b, ..] = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || (a == 100 && (64..=127).contains(&b))
        }
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost"
                || name.ends_with(".local")
                || name.ends_with(".ts.net")
                || name.ends_with(".lan")
                || name.ends_with(".home.arpa")
                || !name.contains('.')
        }
        None => false,
    }
}

pub fn validate_pairing_key(raw: &str) -> Result<SecretKey, String> {
    let decoded = Zeroizing::new(
        B64.decode(raw.trim())
            .map_err(|_| "Link pairing key must be base64".to_string())?,
    );
    let key: [u8; 32] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| "Link pairing key must decode to exactly 32 bytes".to_string())?;
    Ok(Zeroizing::new(key))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Canonical encoding for MAC and KDF input: every field is prefixed with its
/// 4-byte big-endian length, so no two different field lists share bytes.
fn enc(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len() + 4).sum());
    for part in parts {
        out.extend_from_slice(&(part.len() as u32).to_be_bytes());
        out.extend_from_slice(part);
    }
    out
}

fn hello_bytes(node: &str, cn: &[u8; 16], ts: i64, epk: &[u8; 32], cascade: bool) -> Vec<u8> {
    enc(&[
        b"dml2 hello",
        b"2",
        node.as_bytes(),
        cn,
        ts.to_string().as_bytes(),
        epk,
        if cascade { b"1" } else { b"0" },
    ])
}

/// The welcome is bound to a hash of the whole hello, so tampering with any
/// hello field (the cascade flag included) breaks the server's MAC.
fn welcome_bytes(hello: &[u8], sn: &[u8; 16], ts: i64, epk: &[u8; 32]) -> Vec<u8> {
    let hello_hash = Sha256::digest(hello);
    enc(&[
        b"dml2 welcome",
        hello_hash.as_slice(),
        sn,
        ts.to_string().as_bytes(),
        epk,
    ])
}

fn mac(psk: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut hmac = <HmacSha256 as Mac>::new_from_slice(psk).expect("fixed-size HMAC key");
    hmac.update(data);
    hmac.finalize().into_bytes().into()
}

fn verify_mac(psk: &[u8; 32], data: &[u8], encoded: &str) -> Result<(), String> {
    let supplied = B64
        .decode(encoded)
        .map_err(|_| "invalid handshake MAC".to_string())?;
    let mut hmac = <HmacSha256 as Mac>::new_from_slice(psk).expect("fixed-size HMAC key");
    hmac.update(data);
    hmac.verify_slice(&supplied)
        .map_err(|_| "handshake authentication failed".into())
}

/// Session keys: HKDF-SHA256 with the DH result and a pairing key as input
/// keying material and the transcript hash as salt. `labels` separates the
/// primary layer from the cascade layer.
fn derive_keys(
    psk: &[u8; 32],
    shared: &[u8; 32],
    hello: &[u8],
    welcome: &[u8],
    labels: (&[u8], &[u8]),
) -> Result<(SecretKey, SecretKey), String> {
    let salt = Sha256::digest(enc(&[hello, welcome]));
    let mut ikm = Zeroizing::new([0u8; 64]);
    ikm[..32].copy_from_slice(shared);
    ikm[32..].copy_from_slice(psk);
    let hkdf = Hkdf::<Sha256>::new(Some(salt.as_slice()), ikm.as_slice());
    let mut c2s = Zeroizing::new([0u8; 32]);
    let mut s2c = Zeroizing::new([0u8; 32]);
    hkdf.expand(labels.0, c2s.as_mut_slice())
        .map_err(|_| "HKDF c2s failed".to_string())?;
    hkdf.expand(labels.1, s2c.as_mut_slice())
        .map_err(|_| "HKDF s2c failed".to_string())?;
    Ok((c2s, s2c))
}

const PRIMARY_LABELS: (&[u8], &[u8]) = (b"dml2 c2s", b"dml2 s2c");
const CASCADE_LABELS: (&[u8], &[u8]) = (b"dml2 cascade c2s", b"dml2 cascade s2c");

fn decode_exact<const N: usize>(value: Option<&Value>, what: &str) -> Result<[u8; N], String> {
    let text = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("welcome missing {what}"))?;
    B64.decode(text)
        .map_err(|_| format!("invalid {what}"))?
        .try_into()
        .map_err(|_| format!("{what} must be {N} bytes"))
}

struct FrameCodec {
    cipher: Aes256Gcm,
    /// Optional second layer. When present every frame is encrypted twice, with
    /// two different ciphers under two different keys.
    cascade: Option<ChaCha20Poly1305>,
    nonce_prefix: [u8; 4],
    aad: Vec<u8>,
    cascade_aad: Vec<u8>,
    counter: u64,
}

impl FrameCodec {
    fn new(
        key: &[u8; 32],
        node: &str,
        direction: &str,
        cascade_key: Option<&[u8; 32]>,
    ) -> Self {
        let prefix = if direction == "c2s" {
            [1, 0, 0, 0]
        } else {
            [2, 0, 0, 0]
        };
        Self {
            cipher: Aes256Gcm::new_from_slice(key).expect("AES-256 key"),
            cascade: cascade_key
                .map(|k| ChaCha20Poly1305::new_from_slice(k).expect("ChaCha20 key")),
            nonce_prefix: prefix,
            aad: enc(&[b"dml2", node.as_bytes(), direction.as_bytes()]),
            cascade_aad: enc(&[b"dml2 cascade", node.as_bytes(), direction.as_bytes()]),
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
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| "frame counter exhausted".to_string())?;
        let plaintext = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        let nonce = self.nonce(self.counter);
        let mut ciphertext = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &self.aad,
                },
            )
            .map_err(|_| "frame encryption failed".to_string())?;
        if let Some(cascade) = &self.cascade {
            ciphertext = cascade
                .encrypt(
                    chacha20poly1305::Nonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &self.cascade_aad,
                    },
                )
                .map_err(|_| "cascade encryption failed".to_string())?;
        }
        Ok(json!({"t": "e", "n": self.counter, "p": B64.encode(ciphertext)}).to_string())
    }

    fn decrypt(&mut self, frame: &str) -> Result<Value, String> {
        let frame: Value =
            serde_json::from_str(frame).map_err(|_| "invalid encrypted frame JSON".to_string())?;
        if frame.get("t").and_then(Value::as_str) != Some("e") {
            return Err("unencrypted post-handshake frame".into());
        }
        let counter = frame
            .get("n")
            .and_then(Value::as_u64)
            .ok_or_else(|| "missing frame counter".to_string())?;
        if counter <= self.counter {
            return Err("replayed or out-of-order frame".into());
        }
        let mut ciphertext = B64
            .decode(
                frame
                    .get("p")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "missing frame payload".to_string())?,
            )
            .map_err(|_| "invalid frame payload".to_string())?;
        let nonce = self.nonce(counter);
        if let Some(cascade) = &self.cascade {
            ciphertext = cascade
                .decrypt(
                    chacha20poly1305::Nonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &self.cascade_aad,
                    },
                )
                .map_err(|_| "cascade authentication failed".to_string())?;
        }
        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &self.aad,
                },
            )
            .map_err(|_| "frame authentication failed".to_string())?;
        let value = serde_json::from_slice(&plaintext)
            .map_err(|_| "invalid encrypted payload JSON".to_string())?;
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
    let Some(raw_key) = crate::config::get_link_key(&cfg.node_id).map(Zeroizing::new) else {
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
    // Cascade is opt-in and only takes effect when its own key is present.
    let cascade = if cfg.link_cascade {
        match crate::config::get_link_cascade_key(&cfg.node_id) {
            Some(raw) => match validate_pairing_key(&Zeroizing::new(raw)) {
                Ok(key) if *key == *psk => {
                    crate::mqtt::set_status(
                        &app,
                        false,
                        "Cascade key is the same as the pairing key - paste the separate cascade key",
                    );
                    return;
                }
                Ok(key) => Some(key),
                Err(error) => {
                    crate::mqtt::set_status(&app, false, &format!("Cascade key: {error}"));
                    return;
                }
            },
            None => {
                crate::mqtt::set_status(&app, false, "Cascade enabled but its key is missing");
                return;
            }
        }
    } else {
        None
    };
    let (stop_tx, stop_rx) = watch::channel(false);
    *state.stop_tx.lock().await = Some(stop_tx);
    crate::mqtt::set_status(&app, false, "Connecting Link...");

    let app_connection = app.clone();
    let cfg_connection = cfg.clone();
    let mut stop_connection = stop_rx.clone();
    tauri::async_runtime::spawn(async move {
        let mut endpoints = vec![cfg_connection.link_url.clone()];
        if !cfg_connection.link_url_remote.is_empty()
            && cfg_connection.link_url_remote != cfg_connection.link_url
        {
            endpoints.push(cfg_connection.link_url_remote.clone());
        }
        let mut endpoint_index = 0usize;
        let mut failures = 0u32;
        let mut auth_failures = 0u32;
        loop {
            if *stop_connection.borrow() {
                break;
            }
            let label = if endpoints.len() == 1 {
                ""
            } else if endpoint_index == 0 {
                " (local)"
            } else {
                " (remote)"
            };
            crate::mqtt::set_status(
                &app_connection,
                false,
                &format!("Connecting Link{label}..."),
            );
            let session_result = run_session(
                &app_connection,
                &cfg_connection,
                &psk,
                cascade.as_deref(),
                &endpoints[endpoint_index],
                stop_connection.clone(),
            )
            .await;
            *app_connection.state::<AppState>().link_tx.lock().await = None;
            let mut retry_delay = if endpoints.len() > 1 { 2 } else { 5 };
            match session_result {
                Ok(()) if *stop_connection.borrow() => break,
                Ok(()) => {
                    failures = 0;
                    auth_failures = 0;
                }
                Err(error) => {
                    failures += 1;
                    let status = match error.kind {
                        LinkFailure::Auth => format!(
                            "Link rejected{label}: {} (node \"{}\")",
                            error.message, cfg_connection.node_id
                        ),
                        LinkFailure::Locked => {
                            format!("Link locked out{label}: {}", error.message)
                        }
                        LinkFailure::Clock => {
                            format!("Link clock mismatch{label}: {}", error.message)
                        }
                        LinkFailure::Transport => {
                            format!("Link error{label}: {}", error.message)
                        }
                    };
                    crate::mqtt::set_status(&app_connection, false, &status);
                    if error.kind == LinkFailure::Transport {
                        auth_failures = 0;
                    } else {
                        // Nie dobijaj serwera co 2 s - to wpycha node w lockout
                        // i zasypuje log Home Assistanta.
                        retry_delay = auth_retry_delay(error.kind, auth_failures);
                        auth_failures += 1;
                    }
                    if endpoints.len() > 1 && failures >= 2 {
                        endpoint_index = (endpoint_index + 1) % endpoints.len();
                        failures = 0;
                        retry_delay = retry_delay.max(3);
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
                let mut current = collector
                    .take()
                    .unwrap_or_else(crate::sensors::Collector::new);
                let (returned, values, hardware_defs) = tokio::task::spawn_blocking(move || {
                    let (values, hardware_defs) = current.collect(&cfg, connected);
                    (current, values, hardware_defs)
                })
                .await
                .unwrap_or_else(|_| (crate::sensors::Collector::new(), HashMap::new(), Vec::new()));
                collector = Some(returned);
                crate::transport::update_hardware_defs(&app_sensors, hardware_defs).await;
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
    cascade: Option<&[u8; 32]>,
    endpoint: &str,
    mut stop: watch::Receiver<bool>,
) -> Result<(), LinkError> {
    let (mut socket, _) = tokio_tungstenite::connect_async(endpoint)
        .await
        .map_err(classify_connect_error)?;
    let mut cn = [0u8; 16];
    OsRng.fill_bytes(&mut cn);
    let cn_b64 = B64.encode(cn);
    let ts = now_unix();
    // Fresh X25519 key for this session only; dropped (and wiped) right after
    // the shared secret is computed.
    let ephemeral = EphemeralSecret::random_from_rng(OsRng);
    let client_epk = PublicKey::from(&ephemeral).to_bytes();
    let hello_data = hello_bytes(&cfg.node_id, &cn, ts, &client_epk, cascade.is_some());
    let hello = json!({
        "t": "hello",
        "v": PROTOCOL_VERSION,
        "node": cfg.node_id,
        "cn": cn_b64,
        "ts": ts,
        "epk": B64.encode(client_epk),
        // Cascade must match on both ends; Home Assistant rejects a mismatch
        // rather than quietly falling back to the weaker single layer. The flag
        // is covered by the MAC, so nobody on the path can flip it.
        "casc": cascade.is_some(),
        "mac": B64.encode(mac(psk, &hello_data)),
    });
    socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .map_err(|e| e.to_string())?;
    let welcome_message = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .map_err(|_| "welcome timeout".to_string())?
        .ok_or_else(|| "connection closed before welcome".to_string())?
        .map_err(|e| e.to_string())?;
    let welcome_text = welcome_message
        .into_text()
        .map_err(|_| "welcome must be a text frame".to_string())?;
    let welcome: Value = serde_json::from_str(welcome_text.as_str())
        .map_err(|_| "invalid welcome JSON".to_string())?;
    match welcome.get("t").and_then(Value::as_str) {
        Some("welcome") => {}
        // Serwer mowi wprost, dlaczego nie wpuscil - bez tego jedynym sladem
        // bylo zerwane polaczenie i zgadywanie po stronie uzytkownika.
        Some("reject") => {
            return Err(classify_reject(
                welcome.get("reason").and_then(Value::as_str).unwrap_or("auth"),
            ));
        }
        _ => return Err("expected welcome".into()),
    }
    if welcome.get("v").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(classify_reject("version"));
    }
    let sn: [u8; 16] = decode_exact(welcome.get("sn"), "server nonce")?;
    let server_epk: [u8; 32] = decode_exact(welcome.get("epk"), "server key")?;
    let server_ts = welcome
        .get("ts")
        .and_then(Value::as_i64)
        .ok_or_else(|| "welcome missing ts".to_string())?;
    if (now_unix() - server_ts).abs() > MAX_SKEW_SECS {
        return Err(LinkError::new(LinkFailure::Clock, CLOCK_MESSAGE));
    }
    let welcome_data = welcome_bytes(&hello_data, &sn, server_ts, &server_epk);
    verify_mac(
        psk,
        &welcome_data,
        welcome.get("mac").and_then(Value::as_str).unwrap_or(""),
    )?;
    let shared = ephemeral.diffie_hellman(&PublicKey::from(server_epk));
    if !shared.was_contributory() {
        return Err("server sent a low-order X25519 key".into());
    }
    let (c2s, s2c) = derive_keys(psk, shared.as_bytes(), &hello_data, &welcome_data, PRIMARY_LABELS)?;
    let cascade_keys = match cascade {
        Some(key) => Some(derive_keys(
            key,
            shared.as_bytes(),
            &hello_data,
            &welcome_data,
            CASCADE_LABELS,
        )?),
        None => None,
    };
    drop(shared);
    let mut encoder = FrameCodec::new(
        &c2s,
        &cfg.node_id,
        "c2s",
        cascade_keys.as_ref().map(|(c, _)| &**c),
    );
    let mut decoder = FrameCodec::new(
        &s2c,
        &cfg.node_id,
        "s2c",
        cascade_keys.as_ref().map(|(_, s)| &**s),
    );
    drop(cascade_keys);
    let (mut writer, mut reader) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
    *app.state::<AppState>().link_tx.lock().await = Some(out_tx.clone());
    let hardware_defs = app
        .state::<AppState>()
        .hardware_sensor_defs
        .lock()
        .await
        .clone();
    out_tx
        .send(crate::discovery::build_link_declare(cfg, &hardware_defs))
        .map_err(|e| e.to_string())?;
    let cached = app.state::<AppState>().sensor_values.lock().await.clone();
    if !cached.is_empty() {
        out_tx
            .send(json!({"t": "state", "s": typed_states(&cached)}))
            .map_err(|e| e.to_string())?;
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
    if stopped {
        Ok(())
    } else {
        Err("Link websocket closed".into())
    }
}

async fn handle_incoming(app: &AppHandle, tx: &mpsc::UnboundedSender<Value>, payload: Value) {
    match payload.get("t").and_then(Value::as_str) {
        Some("cmd") => {
            let id = payload.get("id").cloned().unwrap_or(Value::Null);
            let key = payload.get("key").and_then(Value::as_str).unwrap_or("");
            let action = payload
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("set");
            let result =
                crate::transport::handle_command(app, key, action, payload.get("value")).await;
            let mut ack = json!({"t": "ack", "id": id, "ok": result.is_ok()});
            if let Err(error) = result {
                ack["error"] = json!(error);
            }
            let _ = tx.send(ack);
        }
        Some("notify") => {
            let id = payload.get("id").cloned().unwrap_or(Value::Null);
            let actions: Vec<Value> =
                payload
                    .get("actions")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items.iter().map(|item| json!({
                    "title": item.get("title").and_then(Value::as_str).unwrap_or(""),
                    "action": item.get("id").and_then(Value::as_str).unwrap_or(""),
                })).collect()
                    })
                    .unwrap_or_default();
            let notification = json!({
                "title": payload.get("title").and_then(Value::as_str).unwrap_or(""),
                "message": payload.get("message").and_then(Value::as_str).unwrap_or(""),
                "image": payload.get("image").cloned().unwrap_or(Value::Null),
                "actions": actions,
            });
            crate::transport::handle_notify(app, &notification.to_string()).await;
            let _ = tx.send(json!({"t": "ack", "id": id, "ok": true}));
        }
        Some("fs") => {
            let id = payload.get("id").cloned().unwrap_or(Value::Null);
            let op = payload.get("op").and_then(Value::as_str).unwrap_or("").to_string();
            let path = payload.get("path").and_then(Value::as_str).unwrap_or("").to_string();
            let cfg = app.state::<AppState>().config.lock().await.clone();
            let response = match tokio::task::spawn_blocking(move || {
                crate::link_files::handle_request(&cfg, &payload)
            })
            .await
            {
                Ok(response) => response,
                Err(_) => {
                    crate::security::audit_file(&op, &path, "error: file worker failed");
                    json!({"t": "fs_res", "id": id, "ok": false, "error": "file worker failed"})
                }
            };
            let _ = tx.send(response);
        }
        Some("ping") => {
            let _ = tx.send(json!({"t": "pong"}));
        }
        Some("pong") => {}
        _ => log::warn!("unknown Deskmate Link payload type"),
    }
}

fn typed_states(values: &HashMap<String, String>) -> Value {
    let mut states = Map::new();
    for (key, value) in values {
        let component = crate::sensors::SENSOR_DEFS
            .iter()
            .find(|definition| definition.id == key)
            .map(|definition| definition.component);
        let typed = match component {
            Some("binary_sensor") => Value::Bool(
                value.eq_ignore_ascii_case("ON")
                    || value.eq_ignore_ascii_case("true")
                    || value == "1",
            ),
            Some("number") => value
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(value.clone())),
            _ if key == "keep_awake" => Value::Bool(value.eq_ignore_ascii_case("ON")),
            _ => Value::String(value.clone()),
        };
        states.insert(key.clone(), typed);
    }
    Value::Object(states)
}

pub async fn send(app: &AppHandle, payload: Value) -> bool {
    app.state::<AppState>()
        .link_tx
        .lock()
        .await
        .as_ref()
        .map(|tx| tx.send(payload).is_ok())
        .unwrap_or(false)
}

pub async fn publish_states_network(app: &AppHandle, values: &HashMap<String, String>) -> usize {
    if send(app, json!({"t": "state", "s": typed_states(values)})).await {
        values.len()
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x25519_dalek::StaticSecret;

    #[test]
    fn normalizes_link_url() {
        assert_eq!(
            normalize_url("ws://ha.local:8123").unwrap(),
            "ws://ha.local:8123/api/deskmate_link/ws"
        );
        assert!(normalize_url("https://ha.local").is_err());
        assert!(normalize_url("wss://user@ha.local").is_err());
    }

    #[test]
    fn plain_ws_only_on_private_hosts() {
        assert!(normalize_url("ws://192.168.1.10:8123").is_ok());
        assert!(normalize_url("ws://100.101.102.103:8123").is_ok());
        assert!(normalize_url("ws://homeassistant:8123").is_ok());
        assert!(normalize_url("ws://ha.tail1234.ts.net:8123").is_ok());
        assert!(normalize_url("ws://ha.example.com").is_err());
        assert!(normalize_url("ws://8.8.8.8:8123").is_err());
        assert!(normalize_url("wss://ha.example.com").is_ok());
    }

    #[test]
    fn encoding_is_unambiguous() {
        assert_ne!(enc(&[b"ab", b"c"]), enc(&[b"a", b"bc"]));
    }

    fn b64<const N: usize>(vector: &Value, key: &str) -> [u8; N] {
        B64.decode(vector[key].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    }

    #[test]
    fn matches_home_assistant_python_v2_vectors() {
        let vector: Value =
            serde_json::from_str(include_str!("../tests/fixtures/deskmate_link_v2.json")).unwrap();
        let psk = validate_pairing_key(vector["psk_b64"].as_str().unwrap()).unwrap();
        let cascade = validate_pairing_key(vector["cascade_b64"].as_str().unwrap()).unwrap();
        let node = vector["node"].as_str().unwrap();
        let cn: [u8; 16] = b64(&vector, "cn_b64");
        let sn: [u8; 16] = b64(&vector, "sn_b64");
        let client_esk = StaticSecret::from(b64::<32>(&vector, "client_esk_b64"));
        let client_epk = PublicKey::from(&client_esk).to_bytes();
        assert_eq!(client_epk, b64::<32>(&vector, "client_epk_b64"));
        let server_epk: [u8; 32] = b64(&vector, "server_epk_b64");

        let hello = hello_bytes(node, &cn, vector["hello_ts"].as_i64().unwrap(), &client_epk, true);
        assert_eq!(B64.encode(mac(&psk, &hello)), vector["hello_mac_b64"].as_str().unwrap());
        let welcome = welcome_bytes(&hello, &sn, vector["welcome_ts"].as_i64().unwrap(), &server_epk);
        verify_mac(&psk, &welcome, vector["welcome_mac_b64"].as_str().unwrap()).unwrap();

        // Flipping the cascade flag must invalidate the MAC chain.
        let tampered = hello_bytes(node, &cn, vector["hello_ts"].as_i64().unwrap(), &client_epk, false);
        assert_ne!(mac(&psk, &tampered), mac(&psk, &hello));

        let shared = client_esk.diffie_hellman(&PublicKey::from(server_epk));
        let (c2s, s2c) = derive_keys(&psk, shared.as_bytes(), &hello, &welcome, PRIMARY_LABELS).unwrap();
        let (cc2s, cs2c) = derive_keys(&cascade, shared.as_bytes(), &hello, &welcome, CASCADE_LABELS).unwrap();
        assert_eq!(B64.encode(*c2s), vector["c2s_key_b64"].as_str().unwrap());
        assert_eq!(B64.encode(*s2c), vector["s2c_key_b64"].as_str().unwrap());
        assert_eq!(B64.encode(*cc2s), vector["cascade_c2s_key_b64"].as_str().unwrap());
        assert_eq!(B64.encode(*cs2c), vector["cascade_s2c_key_b64"].as_str().unwrap());

        let mut decoder = FrameCodec::new(&c2s, node, "c2s", Some(&cc2s));
        let frame = vector["python_c2s_frame"].to_string();
        assert_eq!(decoder.decrypt(&frame).unwrap(), vector["payload"]);
        assert!(
            decoder.decrypt(&frame).is_err(),
            "the same counter must be rejected as replay"
        );
        let mut from_server = FrameCodec::new(&s2c, node, "s2c", Some(&cs2c));
        let ping = vector["python_s2c_ping_frame"].to_string();
        assert_eq!(from_server.decrypt(&ping).unwrap()["t"], "ping");

        // Without the cascade layer the same frame must not authenticate.
        let mut single = FrameCodec::new(&c2s, node, "c2s", None);
        assert!(single.decrypt(&frame).is_err());
    }

    #[test]
    fn rust_frames_round_trip() {
        let key = [7u8; 32];
        let mut encoder = FrameCodec::new(&key, "pc", "c2s", None);
        let mut decoder = FrameCodec::new(&key, "pc", "c2s", None);
        let frame = encoder.encrypt(&json!({"t": "ping"})).unwrap();
        assert_eq!(decoder.decrypt(&frame).unwrap()["t"], "ping");
        let mut other_node = FrameCodec::new(&key, "laptop", "c2s", None);
        assert!(other_node.decrypt(&frame).is_err(), "AAD binds the node");
    }
}
