//! Deskmate Link in the app: the connection loop, the session and what its
//! frames do here. The protocol itself (handshake, frame encryption, URL and
//! key checks, failure kinds) is `deskmate_core::link`; docs/LINK.md describes
//! it.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroizing;

use deskmate_core::link::media::declares_media_player;
use deskmate_core::link::notify::{parse_notify_clear, parse_notify_extras};
use deskmate_core::link::{
    auth_retry_delay, classify_connect_error, client_handshake, feature, info_frame, parse_info,
    state_frame, ClientPlatform, ClientRole, HubInfo, LinkError, LinkFailure, SessionCodecs,
    PING_INTERVAL,
};
pub use deskmate_core::link::{normalize_url, validate_pairing_key};

use crate::config::AppConfig;
use crate::farewell::Farewell;
use crate::state::AppState;
use deskmate_service::runner::bye_frame;

/// Longest wait for the `bye` frame and for the close after it.
const FAREWELL_SEND: Duration = Duration::from_secs(1);

/// The optional protocol features that this app implements, for its `info`
/// frame. Home Assistant uses a feature only with a session that lists it.
const APP_FEATURES: &[&str] = &[feature::MEDIA_PLAYER, feature::NOTIFY_TAG];

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
        crate::mqtt::set_status(&app, false, "Pairing code missing - paste it in Settings");
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
    crate::mqtt::set_status(&app, false, "Connecting to Home Assistant...");

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
            // Another Windows account on this computer may be paired as the
            // same node; only one of them connects at a time.
            if !crate::seat::wait_for_turn(
                &cfg_connection.node_id,
                &mut stop_connection,
                |status| crate::mqtt::set_status(&app_connection, false, status),
            )
            .await
            {
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
                &format!("Connecting to Home Assistant{label}..."),
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
            app_connection.state::<AppState>().set_link_info(None);
            app_connection.state::<AppState>().set_link_farewell(None);
            // Half-written files from Home Assistant cannot be resumed on a
            // new session; remove them now.
            crate::link_files::abort_all();
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
                            "Home Assistant rejected the pairing{label}: {} (node \"{}\")",
                            error.message, cfg_connection.node_id
                        ),
                        LinkFailure::Locked => {
                            format!(
                                "Home Assistant locked this computer out{label}: {}",
                                error.message
                            )
                        }
                        LinkFailure::Clock => {
                            format!(
                                "Clock mismatch with Home Assistant{label}: {}",
                                error.message
                            )
                        }
                        LinkFailure::Transport => {
                            format!(
                                "Connection error{label}: {}",
                                crate::transport::explain_connection_error(&error.message)
                            )
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
                crate::media_player::publish(&app_sensors, false).await;
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
    let SessionCodecs {
        mut encoder,
        mut decoder,
    } = client_handshake(&mut socket, &cfg.node_id, psk, cascade).await?;
    let (mut writer, mut reader) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
    // `info` goes first: it is queued before any other task can reach the
    // channel. An integration older than 0.9.0 ignores it and never answers.
    app.state::<AppState>().set_link_info(None);
    out_tx
        .send(info_frame(
            ClientRole::App,
            ClientPlatform::Windows,
            APP_FEATURES,
        ))
        .map_err(|e| e.to_string())?;
    *app.state::<AppState>().link_tx.lock().await = Some(out_tx.clone());
    let (farewell_tx, mut farewell_rx) = mpsc::unbounded_channel::<Farewell>();
    app.state::<AppState>().set_link_farewell(Some(farewell_tx));
    let hardware_defs = app
        .state::<AppState>()
        .hardware_sensor_defs
        .lock()
        .await
        .clone();
    // Home Assistant has not answered `info` yet, so nothing that needs a
    // cap is declared here; the answer brings a new declaration if it does.
    out_tx
        .send(crate::discovery::build_link_declare(
            cfg,
            &hardware_defs,
            None,
        ))
        .map_err(|e| e.to_string())?;
    let cached = app.state::<AppState>().sensor_values.lock().await.clone();
    if !cached.is_empty() {
        out_tx
            .send(state_frame(&cached))
            .map_err(|e| e.to_string())?;
    }
    crate::mqtt::set_status(app, true, "Connected to Home Assistant");
    log::info!("Deskmate Link connected");
    let mut ping = tokio::time::interval(PING_INTERVAL);
    let mut seat_check = tokio::time::interval(crate::seat::CHECK_EVERY);
    let mut handed_over = false;
    let mut goodbye: Option<std::sync::mpsc::Sender<()>> = None;
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            Some(farewell) = farewell_rx.recv() => {
                // The last frame of the session; the socket closes below.
                if let Ok(frame) = encoder.encrypt(&bye_frame(farewell.reason)) {
                    let _ = tokio::time::timeout(
                        FAREWELL_SEND,
                        writer.send(Message::Text(frame.into())),
                    )
                    .await;
                }
                goodbye = Some(farewell.done);
                break;
            }
            _ = ping.tick() => { let _ = out_tx.send(json!({"t": "ping"})); }
            _ = seat_check.tick() => {
                if !crate::seat::turn(&cfg.node_id) {
                    handed_over = true;
                    break;
                }
            }
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
    app.state::<AppState>().set_link_farewell(None);
    if let Some(done) = goodbye {
        // Windows may end the process right after the goodbye; it waits
        // only a moment for the close.
        let _ = tokio::time::timeout(FAREWELL_SEND, writer.close()).await;
        let _ = done.send(());
        return Ok(());
    }
    let _ = writer.close().await;
    if stopped || handed_over {
        Ok(())
    } else {
        Err("Link websocket closed".into())
    }
}

async fn handle_incoming(app: &AppHandle, tx: &mpsc::UnboundedSender<Value>, payload: Value) {
    match payload.get("t").and_then(Value::as_str) {
        // Commands and file requests run as their own tasks. A confirmation
        // dialog waiting for the user used to stall this loop, the pings went
        // unanswered and Home Assistant dropped the connection.
        Some("cmd") => {
            let app = app.clone();
            let tx = tx.clone();
            tauri::async_runtime::spawn(async move {
                let id = payload.get("id").cloned().unwrap_or(Value::Null);
                let key = payload.get("key").and_then(Value::as_str).unwrap_or("");
                let action = payload
                    .get("action")
                    .and_then(Value::as_str)
                    .unwrap_or("set");
                let result =
                    crate::transport::handle_command(&app, key, action, payload.get("value")).await;
                let mut ack = json!({"t": "ack", "id": id, "ok": result.is_ok()});
                if let Err(error) = result {
                    ack["error"] = json!(error);
                }
                let _ = tx.send(ack);
            });
        }
        Some("fs") => {
            let app = app.clone();
            let tx = tx.clone();
            tauri::async_runtime::spawn(handle_file_request(app, tx, payload));
        }
        Some("notify") => {
            let id = payload.get("id").cloned().unwrap_or(Value::Null);
            let extras = match parse_notify_extras(&payload) {
                Ok(extras) => extras,
                Err(error) => {
                    // Only the fact is logged, never what was received.
                    crate::security::audit("notification", "blocked_invalid_field");
                    log::warn!("refusing a notification: {}", error.message());
                    let _ = tx
                        .send(json!({"t": "ack", "id": id, "ok": false, "error": error.message()}));
                    return;
                }
            };
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
            let mut parsed = crate::notify::parse(&notification.to_string());
            parsed.tag = extras.tag;
            parsed.priority = extras.priority;
            crate::transport::show_notification(app, parsed).await;
            let _ = tx.send(json!({"t": "ack", "id": id, "ok": true}));
        }
        Some("notify_clear") => {
            let app = app.clone();
            let tx = tx.clone();
            tauri::async_runtime::spawn(async move {
                let id = payload.get("id").cloned().unwrap_or(Value::Null);
                let result = match parse_notify_clear(&payload) {
                    Ok(tag) => crate::transport::clear_notification(&app, tag).await,
                    Err(error) => {
                        crate::security::audit("notification_clear", "blocked_invalid_field");
                        log::warn!("refusing to clear a notification: {}", error.message());
                        Err(error.message().to_string())
                    }
                };
                let mut ack = json!({"t": "ack", "id": id, "ok": result.is_ok()});
                if let Err(error) = result {
                    ack["error"] = json!(error);
                }
                let _ = tx.send(ack);
            });
        }
        Some("ping") => {
            let _ = tx.send(json!({"t": "pong"}));
        }
        Some("pong") => {}
        Some("info") => match parse_info(&payload) {
            Some(info) => remember_hub_info(app, info).await,
            // Only the fact is logged, never what was received.
            None => log::warn!("ignoring an invalid info frame from Home Assistant"),
        },
        _ => log::warn!("unknown Deskmate Link payload type"),
    }
}

/// Keeps the `info` answer for the rest of the session and shows the
/// integration's version on the Status page.
async fn remember_hub_info(app: &AppHandle, info: HubInfo) {
    let state = app.state::<AppState>();
    let status = {
        let mut status = state.status.lock().await;
        if !status.connected {
            // The session is already going away.
            return;
        }
        status.integration = Some(info.integration.clone());
        status.clone()
    };
    let media_player = declares_media_player(&*state.config.lock().await, Some(&info));
    state.set_link_info(Some(info));
    let _ = app.emit("deskmate://status", status);
    if media_player {
        // The first declaration went out before this answer and could not
        // include the media player. Its own task: the declaration reads the
        // media session, and the session loop must not wait for that.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::transport::refresh_entities(&app).await;
        });
    }
}

async fn handle_file_request(app: AppHandle, tx: mpsc::UnboundedSender<Value>, payload: Value) {
    let id = payload.get("id").cloned().unwrap_or(Value::Null);
    let op = payload
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let path = payload
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
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
    let received = op == "put_end" && response.get("ok") == Some(&Value::Bool(true));
    let notice = received.then(|| {
        let name = response
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("file");
        let dir = response.get("dir").and_then(Value::as_str).unwrap_or("");
        let size = response.get("size").and_then(Value::as_u64).unwrap_or(0);
        json!({
            "title": "File received from Home Assistant",
            "message": format!("{name} ({:.1} MB) saved to {dir}", size as f64 / 1_048_576.0),
        })
    });
    let _ = tx.send(response);
    if let Some(notice) = notice {
        crate::transport::handle_notify(&app, &notice.to_string()).await;
    }
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
    if send(app, state_frame(values)).await {
        values.len()
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_announces_exactly_what_it_implements() {
        let frame = info_frame(ClientRole::App, ClientPlatform::Windows, APP_FEATURES);
        assert_eq!(frame["features"], json!(["media_player", "notify_tag"]));
    }
}
