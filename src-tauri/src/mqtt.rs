//! MQTT client: connection, LWT, discovery after ConnAck, command and notify
//! routing, sensor loop. Restarted via a watch channel (config change in the UI).

use rumqttc::{
    AsyncClient, Event, LastWill, MqttOptions, Packet, QoS, TlsConfiguration, Transport,
};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

use crate::consts;
use crate::state::{AppState, StatusView};

/// Starts (or restarts) the MQTT connection according to the current config.
pub async fn restart(app: AppHandle) {
    let state = app.state::<AppState>();

    // stop the previous session
    if let Some(tx) = state.stop_tx.lock().await.take() {
        let _ = tx.send(true);
    }
    *state.link_tx.lock().await = None;
    {
        let mut cl = state.client.lock().await;
        if let Some(c) = cl.take() {
            // offline will be sent via LWT once disconnected; also try explicitly
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

    // host list: local (broker_host) always; remote (broker_host_remote) as a
    // fallback when set and different. The client tries them in order; on a
    // failed connection it switches host (local <-> remote), on success it stays.
    let mut hosts: Vec<(String, u16)> = vec![(cfg.broker_host.clone(), cfg.broker_port)];
    let remote = cfg.broker_host_remote.trim().to_string();
    if !remote.is_empty() && remote != cfg.broker_host {
        hosts.push((remote, cfg.broker_port));
    }
    let multi = hosts.len() > 1;
    let creds = if cfg.username.is_empty() {
        None
    } else {
        Some((
            cfg.username.clone(),
            crate::config::get_password(&cfg).unwrap_or_default(),
        ))
    };
    let transport = if cfg.mqtt_transport == "tls" {
        let tls = if cfg.mqtt_ca_path.trim().is_empty() {
            TlsConfiguration::Native
        } else {
            let ca = match std::fs::read(cfg.mqtt_ca_path.trim()) {
                Ok(ca) => ca,
                Err(e) => {
                    set_status(
                        &app,
                        false,
                        &format!("Cannot read MQTT CA certificate: {e}"),
                    );
                    return;
                }
            };
            TlsConfiguration::SimpleNative {
                ca,
                client_auth: None,
            }
        };
        Some(Transport::tls_with_config(tls))
    } else {
        crate::security::audit("mqtt_transport", "insecure");
        None
    };

    set_status(&app, false, "Connecting...");

    // --- MQTT event loop (with host failover) ---
    let app_ev = app.clone();
    let node_ev = node.clone();
    let mut stop_ev = stop_rx.clone();
    tauri::async_runtime::spawn(async move {
        let mut idx = 0usize;
        'hosts: loop {
            let (host, port) = hosts[idx].clone();
            let label = if !multi {
                String::new()
            } else if idx == 0 {
                " (local)".into()
            } else {
                " (remote)".into()
            };

            let mut opts = MqttOptions::new(format!("deskmate-{}", node_ev), host.clone(), port);
            if let Some(transport) = transport.clone() {
                opts.set_transport(transport);
            }
            opts.set_keep_alive(Duration::from_secs(30));
            opts.set_last_will(LastWill::new(
                consts::availability_topic(&node_ev),
                "offline",
                QoS::AtLeastOnce,
                true,
            ));
            if let Some((u, p)) = &creds {
                opts.set_credentials(u.clone(), p.clone());
            }
            let (client, mut eventloop) = AsyncClient::new(opts, 64);
            *app_ev.state::<AppState>().client.lock().await = Some(client.clone());
            set_status(&app_ev, false, &format!("Connecting{label}..."));

            let mut fail_count = 0u32;
            loop {
                tokio::select! {
                    _ = stop_ev.changed() => break 'hosts,
                    ev = eventloop.poll() => match ev {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            fail_count = 0;
                            on_connected(&app_ev, &node_ev).await;
                        }
                        Ok(Event::Incoming(Packet::Publish(p))) => {
                            let topic = p.topic.clone();
                            let payload = String::from_utf8_lossy(&p.payload).to_string();
                            route_incoming(app_ev.clone(), node_ev.clone(), topic, payload, p.retain);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            fail_count += 1;
                            set_status(&app_ev, false, &format!("Connection error{label}: {e}"));
                            // multi-host: after 2 failed attempts, switch to the other host
                            if multi && fail_count >= 2 {
                                idx = (idx + 1) % hosts.len();
                                tokio::time::sleep(Duration::from_secs(3)).await;
                                continue 'hosts;
                            }
                            tokio::time::sleep(Duration::from_secs(if multi { 2 } else { 5 })).await;
                        }
                    }
                }
            }
        }
    });

    // --- sensor loop ---
    let app_sn = app.clone();
    let mut stop_sn = stop_rx;
    tauri::async_runtime::spawn(async move {
        let mut collector: Option<crate::sensors::Collector> = None;
        loop {
            let interval = {
                let st = app_sn.state::<AppState>();
                let cfg = st.config.lock().await.clone();
                let mqtt_connected = st.status.lock().await.connected;
                let secs = cfg.publish_interval_secs.clamp(2, 3600);

                // collection may block (WinAPI/SMTC) - keep it off the executor
                let mut coll = collector
                    .take()
                    .unwrap_or_else(crate::sensors::Collector::new);
                let (coll_back, values, hardware_defs) = tokio::task::spawn_blocking(move || {
                    let (values, hardware_defs) = coll.collect(&cfg, mqtt_connected);
                    (coll, values, hardware_defs)
                })
                .await
                .unwrap_or_else(|_| (crate::sensors::Collector::new(), HashMap::new(), Vec::new()));
                collector = Some(coll_back);

                crate::transport::update_hardware_defs(&app_sn, hardware_defs).await;
                crate::transport::publish_states(&app_sn, &values).await;
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

    let hardware_defs = state.hardware_sensor_defs.lock().await.clone();
    for (topic, payload) in crate::discovery::build_all(&cfg, &hardware_defs) {
        let _ = client.publish(topic, QoS::AtLeastOnce, true, payload).await;
    }

    let _ = client
        .subscribe(
            format!("{}/cmd/+", consts::base_topic(node)),
            QoS::AtLeastOnce,
        )
        .await;
    let _ = client
        .subscribe(consts::notify_topic(node), QoS::AtLeastOnce)
        .await;

    set_status(app, true, "Connected");
    log::info!("MQTT connected, discovery published");
}

/// Publishes sensor states over MQTT. Cache/UI updates are transport-independent.
pub async fn publish_states_network(app: &AppHandle, values: &HashMap<String, String>) -> usize {
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
        return values.len();
    }
    0
}

/// Handles incoming messages (commands + notify).
fn route_incoming(app: AppHandle, node: String, topic: String, payload: String, retained: bool) {
    tauri::async_runtime::spawn(async move {
        let base = consts::base_topic(&node);
        // Commands and notifications are events, never desired state. Accepting a
        // retained message here would replay it after every reconnect/startup.
        if retained
            && (topic == consts::notify_topic(&node) || topic.starts_with(&format!("{base}/cmd/")))
        {
            log::warn!("retained MQTT event ignored: {topic}");
            return;
        }
        if topic == consts::notify_topic(&node) {
            crate::transport::handle_notify(&app, &payload).await;
            return;
        }
        let Some(key) = topic
            .strip_prefix(&format!("{base}/cmd/"))
            .map(|s| s.to_string())
        else {
            return;
        };
        if let Err(error) = crate::transport::handle_command(
            &app,
            &key,
            "set",
            Some(&serde_json::Value::String(payload)),
        )
        .await
        {
            log::warn!("command failed: {error}");
        }
    });
}

/// Publishes the action clicked in the toast to `notify/action` (an HA automation picks it up).
pub async fn publish_action_network(app: &AppHandle, action: &str) {
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

pub(crate) async fn legacy_handle_notify(app: &AppHandle, payload: &str) {
    {
        const WINDOW: Duration = Duration::from_secs(60);
        const LIMIT: usize = 10;
        let state = app.state::<AppState>();
        let mut times = state.notification_times.lock().await;
        let now = std::time::Instant::now();
        while times
            .front()
            .map(|t| now.duration_since(*t) > WINDOW)
            .unwrap_or(false)
        {
            times.pop_front();
        }
        if times.len() >= LIMIT {
            crate::security::audit("notification", "blocked_rate_limit");
            return;
        }
        times.push_back(now);
    }
    let mut parsed = crate::notify::parse(payload);
    if let Some(image) = parsed.image.as_deref() {
        let cfg = app.state::<AppState>().config.lock().await.clone();
        if crate::sys_commands::parse_allowed_web_url(&cfg, image).is_err() {
            parsed.image = None;
            crate::security::audit("notification_image", "blocked_origin");
        }
    }
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
