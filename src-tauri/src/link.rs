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
use deskmate_core::link::retry::RetrySchedule;
use deskmate_core::link::{
    client_handshake, info_frame, open_in_time, parse_info, state_frame, ClientRole, HubInfo,
    LinkError, LinkFailure, SessionCodecs, PING_INTERVAL,
};
pub use deskmate_core::link::{normalize_url, validate_pairing_key};

use crate::config::AppConfig;
use crate::farewell::Farewell;
use crate::state::AppState;
use deskmate_service::runner::bye_frame;

/// Longest wait for the `bye` frame and the close after it, together. It
/// stays well inside the time the app waits for them (`farewell.rs`).
pub(crate) const FAREWELL_SEND: Duration = Duration::from_secs(1);

/// The optional protocol features that this app implements, for its `info`
/// frame. Home Assistant uses a feature only with a session that lists it.
#[cfg(windows)]
const APP_FEATURES: &[&str] = &[
    deskmate_core::link::feature::MEDIA_PLAYER,
    deskmate_core::link::feature::NOTIFY_TAG,
];
/// The Linux preview has the media player (MPRIS). Its notifications are
/// neither replaced by tag nor cleared (linux/notify.rs).
#[cfg(not(windows))]
const APP_FEATURES: &[&str] = &[deskmate_core::link::feature::MEDIA_PLAYER];

/// How long a session waits for Home Assistant's `info` answer before its
/// first `declare`. Home Assistant treats a full declaration as the complete
/// list and removes every entity it does not name, so a declaration made
/// without the answer would remove the media player on each reconnect. An
/// integration older than 0.9.0 never answers and gets the declaration after
/// this wait.
const INFO_ANSWER_WAIT: Duration = Duration::from_secs(3);

/// Where a session stands with its first `declare`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opening {
    AwaitingInfo,
    Declared,
}

/// What a session does with its declaration after an event of the opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclareStep {
    /// Send the first declaration now.
    First,
    /// The answer came after the first declaration: declare again if the
    /// answer adds an entity.
    Again,
    Nothing,
}

impl Opening {
    fn on_info_answer(&mut self) -> DeclareStep {
        match std::mem::replace(self, Self::Declared) {
            Self::AwaitingInfo => DeclareStep::First,
            Self::Declared => DeclareStep::Again,
        }
    }

    fn on_wait_over(&mut self) -> DeclareStep {
        match std::mem::replace(self, Self::Declared) {
            Self::AwaitingInfo => DeclareStep::First,
            Self::Declared => DeclareStep::Nothing,
        }
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
    forget_session(&app).await;

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
        let has_remote = endpoints.len() > 1;
        let mut retry = RetrySchedule::new(endpoints);
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
            let label = retry.label();
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
                retry.endpoint(),
                stop_connection.clone(),
            )
            .await;
            // A loop that `restart` or `transport::stop` replaced may end
            // while the next loop's session is up: the shared state is that
            // session's now, and they forgot this one's already.
            if *stop_connection.borrow() {
                break;
            }
            forget_session(&app_connection).await;
            let retry_delay = match session_result {
                Ok(()) => {
                    retry.reset();
                    retry.base_delay()
                }
                Err(error) => {
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
                        LinkFailure::Unreachable => {
                            format!(
                                "Connection error{label}: {}",
                                crate::transport::explain_unreachable(&error.message, has_remote)
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
                    retry.on_failure(error.kind)
                }
            };
            tokio::select! {
                _ = stop_connection.changed() => break,
                _ = tokio::time::sleep(retry_delay) => {}
            }
        }
        if !*stop_connection.borrow() {
            forget_session(&app_connection).await;
        }
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
    let open = async {
        let (mut socket, _) = open_in_time(tokio_tungstenite::connect_async(endpoint)).await?;
        let codecs = client_handshake(&mut socket, &cfg.node_id, psk, cascade).await?;
        Ok::<_, LinkError>((socket, codecs))
    };
    // A connect to an unreachable address can take seconds to give up. A
    // loop that was replaced in the meantime must not open a session next to
    // the new one.
    let (
        socket,
        SessionCodecs {
            mut encoder,
            mut decoder,
        },
    ) = tokio::select! {
        _ = stop.changed() => return Ok(()),
        opened = open => opened?,
    };
    if *stop.borrow() {
        return Ok(());
    }
    let (mut writer, mut reader) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
    // `info` goes first: it is queued before any other task can reach the
    // channel. An integration older than 0.9.0 ignores it and never answers.
    app.state::<AppState>().set_link_info(None);
    out_tx
        .send(info_frame(
            ClientRole::App,
            crate::platform::CLIENT_PLATFORM,
            APP_FEATURES,
        ))
        .map_err(|e| e.to_string())?;
    let (farewell_tx, mut farewell_rx) = mpsc::unbounded_channel::<Farewell>();
    app.state::<AppState>().set_link_farewell(Some(farewell_tx));
    crate::mqtt::set_status(app, true, "Connected to Home Assistant");
    log::info!("Deskmate Link connected");
    let mut opening = Opening::AwaitingInfo;
    let info_wait = tokio::time::sleep(INFO_ANSWER_WAIT);
    tokio::pin!(info_wait);
    let mut ping = tokio::time::interval(PING_INTERVAL);
    let mut seat_check = tokio::time::interval(crate::seat::CHECK_EVERY);
    let mut handed_over = false;
    let mut farewell: Option<Farewell> = None;
    loop {
        tokio::select! {
            _ = stop.changed() => {
                // A goodbye asked for just before the stop still goes out:
                // the app stops the loop when its wait for the goodbye ran out.
                farewell = farewell_rx.try_recv().ok();
                break;
            }
            _ = &mut info_wait, if opening == Opening::AwaitingInfo => {
                if opening.on_wait_over() == DeclareStep::First {
                    declare_session(app, &out_tx).await?;
                }
            }
            Some(asked) = farewell_rx.recv() => {
                farewell = Some(asked);
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
                        if payload.get("t").and_then(Value::as_str) != Some("info") {
                            handle_incoming(app, &out_tx, payload).await;
                        } else if let Some(info) = parse_info(&payload) {
                            match opening.on_info_answer() {
                                DeclareStep::First => {
                                    remember_hub_info(app, info, false).await;
                                    declare_session(app, &out_tx).await?;
                                }
                                DeclareStep::Again => remember_hub_info(app, info, true).await,
                                DeclareStep::Nothing => {}
                            }
                        } else {
                            // Only the fact is logged, never what was received.
                            log::warn!("ignoring an invalid info frame from Home Assistant");
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(data) => writer.send(Message::Pong(data)).await.map_err(|e| e.to_string())?,
                    _ => {}
                }
            }
        }
    }
    // The last frame of the session; the socket closes below.
    let goodbye = farewell.map(|farewell| (farewell, tokio::time::Instant::now() + FAREWELL_SEND));
    if let Some((farewell, deadline)) = &goodbye {
        if let Ok(frame) = encoder.encrypt(&bye_frame(farewell.reason)) {
            let _ =
                tokio::time::timeout_at(*deadline, writer.send(Message::Text(frame.into()))).await;
        }
    }
    let stopped = *stop.borrow();
    if !stopped {
        // After a stop, what is shared may belong to the next session
        // already; whoever stopped this one has forgotten it.
        *app.state::<AppState>().link_tx.lock().await = None;
        app.state::<AppState>().set_link_farewell(None);
    }
    if let Some((farewell, deadline)) = goodbye {
        // Windows may end the process right after the goodbye; it waits
        // only a moment for the close.
        let _ = tokio::time::timeout_at(deadline, writer.close()).await;
        let _ = farewell.done.send(());
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
        // Inline like `notify`, so that a clear and a notification with the
        // same tag take effect in the order Home Assistant sent them. As a
        // task of its own, a clear could finish after a newer notification
        // and remove it.
        Some("notify_clear") => {
            let id = payload.get("id").cloned().unwrap_or(Value::Null);
            let result = match parse_notify_clear(&payload) {
                Ok(tag) => crate::transport::clear_notification(app, tag).await,
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
        }
        Some("ping") => {
            let _ = tx.send(json!({"t": "pong"}));
        }
        Some("pong") => {}
        _ => log::warn!("unknown Deskmate Link payload type"),
    }
}

/// Sends the first declaration of a session, with what Home Assistant said
/// in its `info` answer if it has answered, and the last known states after
/// it. Other tasks reach the session only from here on, so none of them can
/// declare before it.
///
/// It reads the current settings, not the ones the connection loop started
/// with: the window changes sensors and features without a restart, and Home
/// Assistant removes whatever a full declaration leaves out.
async fn declare_session(
    app: &AppHandle,
    out_tx: &mpsc::UnboundedSender<Value>,
) -> Result<(), LinkError> {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    crate::monitor::brightness::probe_before_declare(&cfg).await;
    let hub = state.link_info();
    let hardware_defs = state.hardware_sensor_defs.lock().await.clone();
    out_tx
        .send(crate::discovery::build_link_declare(
            &cfg,
            &hardware_defs,
            hub.as_ref(),
        ))
        .map_err(|e| e.to_string())?;
    let cached = state.sensor_values.lock().await.clone();
    if !cached.is_empty() {
        out_tx
            .send(state_frame(&cached))
            .map_err(|e| e.to_string())?;
    }
    *state.link_tx.lock().await = Some(out_tx.clone());
    if declares_media_player(&cfg, hub.as_ref()) {
        // Its own task: reading the media session blocks.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::media_player::publish(&app, true).await;
        });
    }
    Ok(())
}

/// Keeps the `info` answer for the rest of the session and shows the
/// integration's version on the Status page. With `redeclare`, the first
/// declaration went out before this answer, so one that adds the media
/// player is sent again.
async fn remember_hub_info(app: &AppHandle, info: HubInfo, redeclare: bool) {
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
    if redeclare && media_player {
        // Its own task: the declaration reads the media session, and the
        // session loop must not wait for that.
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

/// Forgets what a Link session left in the shared state: the channel to it,
/// Home Assistant's `info` answer, the way to its goodbye, and the files it
/// was receiving, which a new session cannot resume.
pub async fn forget_session(app: &AppHandle) {
    let state = app.state::<AppState>();
    *state.link_tx.lock().await = None;
    state.set_link_info(None);
    state.set_link_farewell(None);
    crate::link_files::abort_all();
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
        let frame = info_frame(
            ClientRole::App,
            crate::platform::CLIENT_PLATFORM,
            APP_FEATURES,
        );
        #[cfg(windows)]
        assert_eq!(frame["features"], json!(["media_player", "notify_tag"]));
        #[cfg(not(windows))]
        assert_eq!(frame["features"], json!(["media_player"]));
    }

    /// Home Assistant removes every entity that a full declaration leaves
    /// out, so the first one of a session must wait for the answer that
    /// allows the media player.
    #[test]
    fn the_first_declaration_waits_for_the_info_answer() {
        let mut opening = Opening::AwaitingInfo;
        assert_eq!(opening.on_info_answer(), DeclareStep::First);
        assert_eq!(opening.on_wait_over(), DeclareStep::Nothing);
        assert_eq!(opening, Opening::Declared);
    }

    #[test]
    fn without_an_answer_the_session_declares_once_the_wait_is_over() {
        let mut opening = Opening::AwaitingInfo;
        assert_eq!(opening.on_wait_over(), DeclareStep::First);
        assert_eq!(opening.on_wait_over(), DeclareStep::Nothing);
        // A late answer can still add the media player.
        assert_eq!(opening.on_info_answer(), DeclareStep::Again);
    }
}
