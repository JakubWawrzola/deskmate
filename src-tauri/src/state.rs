//! App state managed by tauri (Manager::state).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicU64;
use tokio::sync::{mpsc, watch, Mutex};

use deskmate_core::link::HubInfo;

use crate::config::AppConfig;

#[derive(Debug, Clone, serde::Serialize)]
pub struct StatusView {
    pub connected: bool,
    pub detail: String,
    /// Version of the Home Assistant integration. Known only while a Link
    /// session is connected and the integration has answered `info`.
    pub integration: Option<String>,
}

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub status: Mutex<StatusView>,
    pub sensor_values: Mutex<HashMap<String, String>>,
    /// Hardware entities detected on this PC. Unsupported telemetry is omitted.
    pub hardware_sensor_defs: Mutex<Vec<crate::sensors::OwnedSensorDef>>,
    pub notif_history: Mutex<VecDeque<crate::notify::NotifyRecord>>,
    pub notification_times: Mutex<VecDeque<std::time::Instant>>,
    pub published_count: AtomicU64,
    pub stop_tx: Mutex<Option<watch::Sender<bool>>>,
    pub client: Mutex<Option<rumqttc::AsyncClient>>,
    /// Outbound encrypted Link payloads for the currently established session.
    pub link_tx: Mutex<Option<mpsc::UnboundedSender<serde_json::Value>>>,
    /// What Home Assistant said about itself in the current Link session.
    /// A plain mutex: it is read where the status is set, outside async code.
    link_info: std::sync::Mutex<Option<HubInfo>>,
    /// toast actions clicked (from the WinRT thread) -> the drain loop publishes to MQTT
    pub action_tx: mpsc::UnboundedSender<String>,
    pub action_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
    /// text to speak via TTS (SAPI thread)
    pub tts_tx: std::sync::mpsc::SyncSender<String>,
    /// sleep-prevention toggle (SetThreadExecutionState thread)
    pub keep_awake_tx: std::sync::mpsc::Sender<bool>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        Self {
            config: Mutex::new(config),
            status: Mutex::new(StatusView {
                connected: false,
                detail: "Starting...".into(),
                integration: None,
            }),
            sensor_values: Mutex::new(HashMap::new()),
            hardware_sensor_defs: Mutex::new(Vec::new()),
            notif_history: Mutex::new(VecDeque::new()),
            notification_times: Mutex::new(VecDeque::new()),
            published_count: AtomicU64::new(0),
            stop_tx: Mutex::new(None),
            client: Mutex::new(None),
            link_tx: Mutex::new(None),
            link_info: std::sync::Mutex::new(None),
            action_tx,
            action_rx: Mutex::new(Some(action_rx)),
            tts_tx: crate::tts::spawn(),
            keep_awake_tx: crate::sys_commands::spawn_keep_awake(),
        }
    }

    /// What Home Assistant said about itself in the current Link session.
    pub fn link_info(&self) -> Option<HubInfo> {
        self.link_info
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Stores the `info` answer of a session, or forgets it with `None` when
    /// a session starts or ends.
    pub fn set_link_info(&self, info: Option<HubInfo>) {
        *self.link_info.lock().unwrap_or_else(|e| e.into_inner()) = info;
    }
}
