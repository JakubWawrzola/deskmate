//! Klient MQTT: polaczenie, LWT, discovery po ConnAck, routing komend i notify,
//! petla sensorow. Restart przez watch-channel (zmiana configu w UI).

use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

use crate::consts;
use crate::state::{AppState, StatusView};

/// Startuje (lub restartuje) polaczenie MQTT wg aktualnego configu.
pub async fn restart(app: AppHandle) {
    let state = app.state::<AppState>();

    // zatrzymaj poprzednia sesje
    if let Some(tx) = state.stop_tx.lock().await.take() {
        let _ = tx.send(true);
    }
    {
        let mut cl = state.client.lock().await;
        if let Some(c) = cl.take() {
            // wysle offline przez LWT po zerwaniu; jawnie tez sprobujemy
            let _ = c.disconnect().await;
        }
    }

    let cfg = state.config.lock().await.clone();
    if !cfg.configured || cfg.broker_host.is_empty() {
        set_status(&app, false, "Not configured");
        return;
    }

    let (stop_tx, stop_rx) = watch::channel(false);
    *state.stop_tx.lock().await = Some(stop_tx);

    let node = cfg.node_id.clone();
    let mut opts = MqttOptions::new(
        format!("deskmate-{}", node),
        cfg.broker_host.clone(),
        cfg.broker_port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_last_will(LastWill::new(
        consts::availability_topic(&node),
        "offline",
        QoS::AtLeastOnce,
        true,
    ));
    if !cfg.username.is_empty() {
        let password = crate::config::get_password(&cfg).unwrap_or_default();
        opts.set_credentials(cfg.username.clone(), password);
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 64);
    *state.client.lock().await = Some(client.clone());
    set_status(&app, false, "Connecting...");

    // --- petla zdarzen MQTT ---
    let app_ev = app.clone();
    let node_ev = node.clone();
    let mut stop_ev = stop_rx.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = stop_ev.changed() => break,
                ev = eventloop.poll() => match ev {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        on_connected(&app_ev, &node_ev).await;
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        let topic = p.topic.clone();
                        let payload = String::from_utf8_lossy(&p.payload).to_string();
                        route_incoming(app_ev.clone(), node_ev.clone(), topic, payload);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        set_status(&app_ev, false, &format!("Connection error: {e}"));
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }
    });

    // --- petla sensorow ---
    let app_sn = app.clone();
    let mut stop_sn = stop_rx;
    tauri::async_runtime::spawn(async move {
        let mut collector: Option<crate::sensors::Collector> = None;
        loop {
            let interval = {
                let st = app_sn.state::<AppState>();
                let cfg = st.config.lock().await.clone();
                let secs = cfg.publish_interval_secs.clamp(2, 3600);

                // zbieranie potencjalnie blokujace (WinAPI/SMTC) - poza executorem
                let mut coll = collector.take().unwrap_or_else(crate::sensors::Collector::new);
                let (coll_back, values) = tokio::task::spawn_blocking(move || {
                    let v = coll.collect(&cfg);
                    (coll, v)
                })
                .await
                .unwrap_or_else(|_| (crate::sensors::Collector::new(), HashMap::new()));
                collector = Some(coll_back);

                publish_states(&app_sn, &values).await;
                secs
            };
            tokio::select! {
                _ = stop_sn.changed() => break,
                _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
            }
        }
    });
}

async fn on_connected(app: &AppHandle, node: &str) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    let client = { state.client.lock().await.clone() };
    let Some(client) = client else { return };

    let _ = client
        .publish(
            consts::availability_topic(node),
            QoS::AtLeastOnce,
            true,
            "online",
        )
        .await;

    for (topic, payload) in crate::discovery::build_all(&cfg) {
        let _ = client.publish(topic, QoS::AtLeastOnce, true, payload).await;
    }

    let _ = client
        .subscribe(format!("{}/cmd/+", consts::base_topic(node)), QoS::AtLeastOnce)
        .await;
    let _ = client
        .subscribe(consts::notify_topic(node), QoS::AtLeastOnce)
        .await;

    set_status(app, true, "Connected");
    log::info!("MQTT connected, discovery published");
}

/// Publikacja stanow sensorow + cache + event do UI.
pub async fn publish_states(app: &AppHandle, values: &HashMap<String, String>) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    let client = { state.client.lock().await.clone() };
    if let Some(client) = client {
        for (key, val) in values {
            let _ = client
                .publish(
                    consts::state_topic(&cfg.node_id, key),
                    QoS::AtMostOnce,
                    false,
                    val.clone(),
                )
                .await;
        }
        state
            .published_count
            .fetch_add(values.len() as u64, Ordering::Relaxed);
    }
    {
        let mut cache = state.sensor_values.lock().await;
        for (k, v) in values {
            cache.insert(k.clone(), v.clone());
        }
    }
    let _ = app.emit("deskmate://sensors", values.clone());
}

/// Obsluga przychodzacych wiadomosci (komendy + notify).
fn route_incoming(app: AppHandle, node: String, topic: String, payload: String) {
    tauri::async_runtime::spawn(async move {
        let base = consts::base_topic(&node);
        if topic == consts::notify_topic(&node) {
            handle_notify(&app, &payload).await;
            return;
        }
        let Some(key) = topic.strip_prefix(&format!("{base}/cmd/")).map(|s| s.to_string()) else {
            return;
        };
        log::info!("command from HA: {key}");
        let state = app.state::<AppState>();
        let cfg = state.config.lock().await.clone();

        // --- klawiatura (wpis tekstu / prezentacja): opt-in allow_input ---
        let is_input = key == "type_text" || key.starts_with("present_");
        if is_input && !cfg.allow_input {
            log::warn!("input command '{key}' zignorowane (allow_input off)");
            return;
        }

        // --- TTS: opt-in tts_enabled ---
        if key == "tts_say" {
            if cfg.tts_enabled {
                let _ = state.tts_tx.send(payload.clone());
            }
            return;
        }

        // --- schowek: ustaw z HA (gdy bridge schowka wlaczony) ---
        if key == "clipboard_set" {
            if crate::sensors::is_enabled(&cfg, "clipboard") {
                let p = payload.clone();
                let _ = tokio::task::spawn_blocking(move || crate::clipboard::set_text(&p)).await;
                if let Ok(Some(c)) =
                    tokio::task::spawn_blocking(crate::clipboard::get_text).await
                {
                    let mut m = HashMap::new();
                    m.insert("clipboard".to_string(), c.chars().take(240).collect());
                    publish_states(&app, &m).await;
                }
            }
            return;
        }

        // --- custom kontrolka (button/switch/number): wartosc -> $env:DESKMATE_VALUE ---
        if let Some(id) = key.strip_prefix("custom_").map(|s| s.to_string()) {
            let cfg2 = cfg.clone();
            let val = payload.clone();
            let r = tokio::task::spawn_blocking(move || {
                crate::sys_commands::run_custom(&cfg2, &id, &val)
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            if let Err(e) = r {
                log::warn!("custom command failed: {e}");
            }
            return;
        }

        // --- builtin (lock/media/volume/type_text/present_* po gate) ---
        let key2 = key.clone();
        let pl = payload.clone();
        let result = tokio::task::spawn_blocking(move || crate::sys_commands::run_builtin(&key2, &pl))
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
        if let Err(e) = result {
            log::warn!("command failed: {e}");
        }
        if key == "volume" {
            if let Some(v) = tokio::task::spawn_blocking(crate::sys_commands::get_volume)
                .await
                .ok()
                .flatten()
            {
                let mut m = HashMap::new();
                m.insert("volume".to_string(), v.to_string());
                publish_states(&app, &m).await;
            }
        }
    });
}

/// Publikuje akcje klikniete w toascie na `notify/action` (HA lapie automatyzacja).
pub async fn publish_action(app: &AppHandle, action: &str) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    let client = { state.client.lock().await.clone() };
    if let Some(client) = client {
        let _ = client
            .publish(
                consts::notify_action_topic(&cfg.node_id),
                QoS::AtLeastOnce,
                false,
                serde_json::json!({ "action": action }).to_string(),
            )
            .await;
    }
}

async fn handle_notify(app: &AppHandle, payload: &str) {
    let parsed = crate::notify::parse(payload);
    let record = crate::notify::NotifyRecord {
        title: parsed.title.clone(),
        message: parsed.message.clone(),
        image: parsed.image.clone(),
        received_at: now_hms(),
    };
    {
        let state = app.state::<AppState>();
        let mut hist = state.notif_history.lock().await;
        hist.push_front(record.clone());
        hist.truncate(50);
    }
    let _ = app.emit("deskmate://notify", record);
    let action_tx = { app.state::<AppState>().action_tx.clone() };
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::notify::show_toast(&parsed, Some(action_tx)) {
            log::warn!("toast failed: {e}");
        }
    })
    .await;
}

#[cfg(windows)]
fn now_hms() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}
#[cfg(not(windows))]
fn now_hms() -> String {
    String::new()
}

pub fn set_status(app: &AppHandle, connected: bool, detail: &str) {
    let state = app.state::<AppState>();
    if let Ok(mut st) = state.status.try_lock() {
        st.connected = connected;
        st.detail = detail.to_string();
    }
    let _ = app.emit(
        "deskmate://status",
        StatusView {
            connected,
            detail: detail.to_string(),
        },
    );
}
