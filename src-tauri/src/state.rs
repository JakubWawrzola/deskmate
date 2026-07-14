//! Stan aplikacji zarzadzany przez tauri (Manager::state).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicU64;
use tokio::sync::{mpsc, watch, Mutex};

use crate::config::AppConfig;

#[derive(Debug, Clone, serde::Serialize)]
pub struct StatusView {
    pub connected: bool,
    pub detail: String,
}

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub status: Mutex<StatusView>,
    pub sensor_values: Mutex<HashMap<String, String>>,
    pub notif_history: Mutex<VecDeque<crate::notify::NotifyRecord>>,
    pub published_count: AtomicU64,
    pub stop_tx: Mutex<Option<watch::Sender<bool>>>,
    pub client: Mutex<Option<rumqttc::AsyncClient>>,
    /// akcje klikniete w toascie (z watku WinRT) -> drain publikuje na MQTT
    pub action_tx: mpsc::UnboundedSender<String>,
    pub action_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
    /// teksty do wypowiedzenia przez TTS (watek SAPI)
    pub tts_tx: std::sync::mpsc::Sender<String>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        Self {
            config: Mutex::new(config),
            status: Mutex::new(StatusView {
                connected: false,
                detail: "Starting...".into(),
            }),
            sensor_values: Mutex::new(HashMap::new()),
            notif_history: Mutex::new(VecDeque::new()),
            published_count: AtomicU64::new(0),
            stop_tx: Mutex::new(None),
            client: Mutex::new(None),
            action_tx,
            action_rx: Mutex::new(Some(action_rx)),
            tts_tx: crate::tts::spawn(),
        }
    }
}
