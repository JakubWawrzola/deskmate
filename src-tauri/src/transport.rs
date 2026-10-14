//! Transport-independent command handling, state publication and lifecycle.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// Adds what to do to the Windows socket errors people actually hit. The raw
/// text ("An attempt was made to access a socket in a way forbidden by its
/// access permissions. (os error 10013)") reads like a Deskmate bug, but it is
/// almost always a firewall or antivirus blocking deskmate.exe.
pub fn explain_connection_error(message: &str) -> String {
    let hint = if message.contains("os error 10013") {
        "Windows blocked the connection. A firewall or antivirus (for example Bitdefender) \
         is probably blocking deskmate.exe: allow it there, then press Reconnect"
    } else if message.contains("os error 10061") {
        "Nothing answered at that address and port. Check the Home Assistant address and port"
    } else if message.contains("os error 10060") || message.contains("os error 10065") {
        "Home Assistant could not be reached. Check that this computer is on the same network \
         or VPN and that the address is right"
    } else {
        return message.to_string();
    };
    format!("{hint} ({message})")
}

pub async fn restart(app: AppHandle) {
    let transport = app
        .state::<AppState>()
        .config
        .lock()
        .await
        .transport
        .clone();
    if transport == "link" {
        crate::link::restart(app).await;
    } else {
        crate::mqtt::restart(app).await;
    }
}

pub async fn publish_states(app: &AppHandle, values: &HashMap<String, String>) {
    let transport = app
        .state::<AppState>()
        .config
        .lock()
        .await
        .transport
        .clone();
    let sent = if transport == "link" {
        crate::link::publish_states_network(app, values).await
    } else {
        crate::mqtt::publish_states_network(app, values).await
    };
    let state = app.state::<AppState>();
    if sent > 0 {
        state
            .published_count
            .fetch_add(sent as u64, Ordering::Relaxed);
    }
    {
        let mut cache = state.sensor_values.lock().await;
        for (key, value) in values {
            cache.insert(key.clone(), value.clone());
        }
    }
    let _ = app.emit("deskmate://sensors", values.clone());
}

pub async fn update_hardware_defs(app: &AppHandle, defs: Vec<crate::sensors::OwnedSensorDef>) {
    // A sound device plugged in or removed changes the select options.
    let devices_changed = crate::audio::take_changed();
    let (changed, removed) = {
        let state = app.state::<AppState>();
        let mut current = state.hardware_sensor_defs.lock().await;
        if *current == defs {
            (false, Vec::new())
        } else {
            let ids: std::collections::HashSet<&str> =
                defs.iter().map(|def| def.id.as_str()).collect();
            let removed = current
                .iter()
                .filter(|def| !ids.contains(def.id.as_str()))
                .map(|def| def.id.clone())
                .collect();
            *current = defs;
            (true, removed)
        }
    };
    if changed {
        let state = app.state::<AppState>();
        {
            let mut cache = state.sensor_values.lock().await;
            for id in &removed {
                cache.remove(id);
            }
        }
        let cfg = state.config.lock().await.clone();
        if cfg.transport == "mqtt" {
            if let Some(client) = state.client.lock().await.clone() {
                for id in &removed {
                    let (topic, payload) = crate::discovery::remove_hardware(&cfg.node_id, id);
                    let _ = client
                        .publish(topic, rumqttc::QoS::AtLeastOnce, true, payload)
                        .await;
                }
            }
        }
        refresh_entities(app).await;
    } else if devices_changed {
        refresh_entities(app).await;
    }
}

pub async fn publish_action(app: &AppHandle, action: &str) {
    let transport = app
        .state::<AppState>()
        .config
        .lock()
        .await
        .transport
        .clone();
    if transport == "link" {
        crate::link::send(
            app,
            serde_json::json!({"t": "notify_action", "action": action}),
        )
        .await;
    } else {
        crate::mqtt::publish_action_network(app, action).await;
    }
}

pub async fn publish_trigger(app: &AppHandle, hotkey_id: &str) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    if cfg.transport == "link" {
        crate::link::send(
            app,
            serde_json::json!({
                "t": "trigger",
                "key": crate::discovery::link_hotkey_key(hotkey_id),
                "event": "press",
            }),
        )
        .await;
    } else if let Some(client) = state.client.lock().await.clone() {
        let topic = format!(
            "{}/hotkey/{}",
            crate::consts::base_topic(&cfg.node_id),
            hotkey_id
        );
        let _ = client
            .publish(topic, rumqttc::QoS::AtLeastOnce, false, "PRESS")
            .await;
    }
}

pub async fn refresh_entities(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    let hardware_defs = state.hardware_sensor_defs.lock().await.clone();
    if cfg.transport == "link" {
        crate::link::send(
            app,
            crate::discovery::build_link_declare(&cfg, &hardware_defs),
        )
        .await;
    } else if let Some(client) = state.client.lock().await.clone() {
        for (topic, payload) in crate::discovery::build_all(&cfg, &hardware_defs) {
            let _ = client
                .publish(topic, rumqttc::QoS::AtLeastOnce, true, payload)
                .await;
        }
    }
}

pub async fn handle_notify(app: &AppHandle, payload: &str) {
    crate::mqtt::legacy_handle_notify(app, payload).await;
}

fn payload_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => {
            if *value {
                "ON".into()
            } else {
                "OFF".into()
            }
        }
        Some(Value::Number(value)) => value.to_string(),
        Some(value) => value.to_string(),
        None => "PRESS".into(),
    }
}

pub async fn handle_command(
    app: &AppHandle,
    key: &str,
    action: &str,
    value: Option<&Value>,
) -> Result<(), String> {
    if key.is_empty() || !matches!(action, "press" | "set") {
        return Err("invalid command envelope".into());
    }
    let payload = payload_string(value);
    log::info!("command from HA: {key}");
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();

    let is_input = key == "type_text" || key == "open_url" || key.starts_with("present_");
    if is_input && !cfg.allow_input {
        return Err(format!("input command '{key}' disabled"));
    }

    if key == "keep_awake" {
        let on = payload.trim().eq_ignore_ascii_case("ON");
        state.keep_awake_tx.send(on).map_err(|e| e.to_string())?;
        if cfg.transport == "mqtt" {
            if let Some(client) = state.client.lock().await.clone() {
                client
                    .publish(
                        crate::consts::state_topic(&cfg.node_id, "keep_awake"),
                        rumqttc::QoS::AtLeastOnce,
                        true,
                        if on { "ON" } else { "OFF" },
                    )
                    .await
                    .map_err(|e| e.to_string())?;
            }
        } else {
            let mut values = HashMap::new();
            values.insert(
                "keep_awake".to_string(),
                if on { "ON".into() } else { "OFF".into() },
            );
            publish_states(app, &values).await;
        }
        return Ok(());
    }

    if key == "audio_mute" || key == "mic_mute" {
        let on = payload.trim().eq_ignore_ascii_case("ON");
        if key == "audio_mute" {
            crate::sys_commands::set_audio_muted(on)?;
        } else {
            tokio::task::spawn_blocking(move || crate::audio::set_mic_muted(on))
                .await
                .map_err(|e| e.to_string())??;
        }
        // The sensor loop would pick this up on its next tick anyway; echoing it
        // immediately keeps the switch from snapping back in the HA interface.
        let mut values = HashMap::new();
        values.insert(
            key.to_string(),
            if on { "ON".into() } else { "OFF".to_string() },
        );
        publish_states(app, &values).await;
        return Ok(());
    }

    if crate::family::is_command(key) {
        return crate::family::handle_command(app, key, &payload).await;
    }

    if key == "tts_say" {
        if !cfg.tts_enabled {
            return Err("TTS disabled".into());
        }
        let text: String = payload.chars().take(1_000).collect();
        return state.tts_tx.try_send(text).map_err(|e| e.to_string());
    }

    if key == "clipboard_set" {
        if cfg.clipboard_write_mode == "off" {
            return Err("clipboard writes disabled".into());
        }
        const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;
        if payload.len() > MAX_CLIPBOARD_BYTES {
            crate::security::audit("clipboard_write", "blocked_size");
            return Err("clipboard payload too large".into());
        }
        if crate::sensors::session_locked() {
            crate::security::audit("clipboard_write", "blocked_locked");
            return Err("session locked".into());
        }
        if !crate::clipboard::claim_write_slot() {
            crate::security::audit("clipboard_write", "blocked_cooldown");
            return Err("clipboard write cooldown".into());
        }
        if cfg.clipboard_write_mode == "confirm" {
            let preview = crate::security::safe_preview(&payload, 160);
            let approved = tokio::task::spawn_blocking(move || {
                crate::security::confirm(
                    "Deskmate clipboard write",
                    &format!("Home Assistant wants to replace the clipboard with:\n\n{preview}\n\nAllow once?"),
                )
            })
            .await
            .unwrap_or(false);
            crate::security::audit(
                "clipboard_write_confirmation",
                if approved { "approved" } else { "denied" },
            );
            if !approved {
                return Err("clipboard write denied".into());
            }
        } else if cfg.clipboard_write_mode != "automatic" {
            return Err("invalid clipboard mode".into());
        }
        let text = payload.clone();
        tokio::task::spawn_blocking(move || crate::clipboard::set_text(&text))
            .await
            .map_err(|e| e.to_string())??;
        crate::security::audit("clipboard_write", "completed");
        return Ok(());
    }

    if key == "open_url" {
        let cfg2 = cfg.clone();
        let url = payload.clone();
        let result =
            tokio::task::spawn_blocking(move || crate::sys_commands::open_allowed_url(&cfg2, &url))
                .await
                .map_err(|e| e.to_string())?;
        crate::security::audit(
            "open_url",
            if result.is_ok() { "allowed" } else { "blocked" },
        );
        return result;
    }

    if let Some(id) = key.strip_prefix("custom_") {
        let cfg2 = cfg.clone();
        let id = id.to_string();
        return tokio::task::spawn_blocking(move || {
            crate::sys_commands::run_custom(&cfg2, &id, &payload)
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    let key_owned = key.to_string();
    let selected = payload.trim().to_string();
    let result =
        tokio::task::spawn_blocking(move || crate::sys_commands::run_builtin(&key_owned, &payload))
            .await
            .map_err(|e| e.to_string())?;
    result?;
    // Echo the new value at once, so the control does not snap back in Home
    // Assistant until the next sensor tick.
    let echoed = match key {
        "volume" => tokio::task::spawn_blocking(crate::sys_commands::get_volume)
            .await
            .ok()
            .flatten()
            .map(|v| v.to_string()),
        "mic_volume" => tokio::task::spawn_blocking(crate::audio::mic_volume)
            .await
            .ok()
            .flatten()
            .map(|v| v.to_string()),
        "audio_output" | "audio_input" => Some(selected),
        _ => None,
    };
    if let Some(value) = echoed {
        let mut values = HashMap::new();
        values.insert(key.to_string(), value);
        publish_states(app, &values).await;
    }
    Ok(())
}
