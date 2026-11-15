//! The channel inside the running app: Home Assistant through `ha_api`,
//! actions through `actions::execute`, the audit log, and the state Tauri
//! manages.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use super::action_ids::{self, ActionSource};
use super::backend::{
    ActionEntry, ActionRunner, AuditSink, ChannelState, ChannelStatus, EntityInfo, HaFailure,
    HomeAssistant, ServiceCall,
};
use super::entities;
use crate::config::{self, AppConfig};
use crate::ha_api;
use crate::state::AppState;

/// Sent to the window whenever the channel's status changes.
const STATUS_EVENT: &str = "deskmate://streamdeck";

/// Writes the channel's audit records to the security log, in order, on a
/// thread of its own: the log is a file, and the channel runs on the async
/// runtime. The thread starts with the first record.
#[derive(Default)]
struct SecurityLog {
    records: std::sync::OnceLock<std::sync::mpsc::Sender<(String, String)>>,
}

impl AuditSink for SecurityLog {
    fn record(&self, event: &str, result: &str) {
        let records = self.records.get_or_init(|| {
            let (records, incoming) = std::sync::mpsc::channel::<(String, String)>();
            std::thread::spawn(move || {
                for (event, result) in incoming {
                    crate::security::audit(&event, &result);
                }
            });
            records
        });
        // The thread ends only with the app; a record that cannot be handed
        // over then has nowhere to go.
        let _ = records.send((event.to_string(), result.to_string()));
    }
}

/// Home Assistant over the REST channel, with the address and the token the
/// app already has.
struct AppHomeAssistant {
    app: AppHandle,
}

impl AppHomeAssistant {
    /// Runs a blocking `ha_api` call on a blocking task, with the current
    /// configuration. `what` names the call in the app's log; without it a
    /// failure is not logged.
    async fn blocking<T: Send + 'static>(
        &self,
        what: Option<&'static str>,
        call: impl FnOnce(&AppConfig) -> Result<T, String> + Send + 'static,
    ) -> Result<T, HaFailure> {
        let cfg = current_config(&self.app).await;
        match tokio::task::spawn_blocking(move || call(&cfg)).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                if let Some(what) = what {
                    log::warn!("streamdeck: {what}: {error}");
                }
                Err(HaFailure)
            }
            Err(_) => Err(HaFailure),
        }
    }
}

impl HomeAssistant for AppHomeAssistant {
    async fn is_configured(&self) -> bool {
        // Reads Credential Manager, so it runs on a blocking task as well.
        self.blocking(None, |cfg| Ok(ha_api::is_configured(cfg)))
            .await
            .unwrap_or(false)
    }

    async fn entities(&self) -> Result<Vec<EntityInfo>, HaFailure> {
        self.blocking(Some("listing the entities"), ha_api::get_all_states)
            .await
            .map(entities::from_states)
    }

    async fn states(&self, entity_ids: &[String]) -> Result<HashMap<String, String>, HaFailure> {
        let ids = entity_ids.to_vec();
        // Asked every few seconds while Home Assistant is away, so a failure
        // is not logged each time.
        self.blocking(None, move |cfg| {
            let states = ha_api::get_states_of(cfg, &ids)?;
            Ok(ids
                .into_iter()
                .zip(states)
                .filter_map(|(entity_id, state)| Some((entity_id, state?)))
                .collect())
        })
        .await
    }

    async fn toggle(&self, entity_id: &str) -> Result<(), HaFailure> {
        let entity_id = entity_id.to_string();
        self.blocking(Some("toggle"), move |cfg| ha_api::toggle(cfg, &entity_id))
            .await
    }

    async fn call_service(&self, call: ServiceCall) -> Result<(), HaFailure> {
        self.blocking(Some("service call"), move |cfg| {
            ha_api::call_service(
                cfg,
                &call.domain,
                &call.service,
                call.entity_id.as_deref(),
                &serde_json::Value::Object(call.data),
            )
        })
        .await
    }
}

/// The hotkeys and tray quick actions of the configuration, run by the
/// executor that the hotkeys and the tray use.
struct AppActions {
    app: AppHandle,
}

impl ActionRunner for AppActions {
    async fn actions(&self) -> Vec<ActionEntry> {
        action_ids::list(&current_config(&self.app).await)
    }

    async fn run(&self, action_id: &str) -> bool {
        let cfg = current_config(&self.app).await;
        let Some(action) = action_ids::find(&cfg, action_id) else {
            return false;
        };
        // What a press of the hotkey itself does (hotkeys.rs): over Link every
        // hotkey is also an event entity in Home Assistant.
        if action.source == ActionSource::Hotkey
            && cfg.transport == "link"
            && action.spec.kind != "mqtt"
        {
            crate::transport::publish_trigger(&self.app, &action.source_id).await;
        }
        crate::actions::execute(&self.app, &action.spec, &action.source_id).await;
        true
    }
}

async fn current_config(app: &AppHandle) -> AppConfig {
    app.state::<AppState>().config.lock().await.clone()
}

/// The channel of the running app, managed by Tauri.
pub struct StreamDeck {
    audit: Arc<dyn AuditSink>,
    /// The name of the pipe could not be worked out the last time the
    /// channel was switched on.
    start_failed: AtomicBool,
    #[cfg(windows)]
    channel: std::sync::Mutex<Option<super::server::Channel>>,
}

impl StreamDeck {
    fn new() -> Self {
        Self {
            audit: Arc::new(SecurityLog::default()),
            start_failed: AtomicBool::new(false),
            #[cfg(windows)]
            channel: std::sync::Mutex::new(None),
        }
    }

    /// Opens the channel or closes it, with all its connections. Has to be
    /// called inside the async runtime.
    #[cfg(windows)]
    fn switch(&self, app: &AppHandle, enabled: bool) {
        use super::backend::{Backend, Timing};
        use super::pipe_name::{current_user_name, pipe_path_for_user};
        use super::server::Channel;

        let mut channel = self.channel.lock().unwrap_or_else(|e| e.into_inner());
        self.start_failed.store(false, Ordering::SeqCst);
        if !enabled {
            // Dropping the channel closes the pipe and every connection.
            *channel = None;
            return;
        }
        if channel.is_some() {
            return;
        }
        let pipe_path = match current_user_name() {
            Ok(user_name) => pipe_path_for_user(&user_name),
            Err(error) => {
                log::warn!("streamdeck: cannot read the Windows user name: {error}");
                self.start_failed.store(true, Ordering::SeqCst);
                return;
            }
        };
        let backend = Arc::new(Backend {
            home_assistant: AppHomeAssistant { app: app.clone() },
            actions: AppActions { app: app.clone() },
            audit: Arc::clone(&self.audit),
            app_version: app.package_info().version.to_string(),
            timing: Timing::default(),
        });
        let window = app.clone();
        *channel = Some(Channel::start(pipe_path, backend, move || {
            let _ = window.emit(STATUS_EVENT, ());
        }));
    }

    #[cfg(not(windows))]
    fn switch(&self, _app: &AppHandle, enabled: bool) {
        // The channel is a Windows named pipe.
        self.start_failed.store(enabled, Ordering::SeqCst);
    }

    fn channel_status(&self) -> ChannelStatus {
        #[cfg(windows)]
        if let Some(channel) = self
            .channel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return channel.status();
        }
        ChannelStatus {
            state: if self.start_failed.load(Ordering::SeqCst) {
                ChannelState::Failed
            } else {
                ChannelState::Off
            },
            connected: 0,
        }
    }

    fn close_connections(&self) {
        #[cfg(windows)]
        if let Some(channel) = self
            .channel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            channel.close_connections();
        }
    }
}

/// What the Settings panel shows.
#[derive(serde::Serialize)]
pub struct StreamDeckView {
    /// The switch as stored in the configuration.
    enabled: bool,
    channel: ChannelStatus,
}

/// Registers the channel with the app and opens it when the configuration
/// has it switched on.
pub fn init(app: &AppHandle) {
    app.manage(StreamDeck::new());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if current_config(&app).await.streamdeck_enabled {
            app.state::<StreamDeck>().switch(&app, true);
        }
    });
}

/// The Home Assistant address or token changed: the connections are closed,
/// so that every client reconnects and reads `ha` again.
pub fn ha_settings_changed(app: &AppHandle) {
    if let Some(stream_deck) = app.try_state::<StreamDeck>() {
        stream_deck.close_connections();
    }
}

pub async fn view(app: &AppHandle) -> Result<StreamDeckView, String> {
    Ok(StreamDeckView {
        enabled: current_config(app).await.streamdeck_enabled,
        channel: app.state::<StreamDeck>().channel_status(),
    })
}

pub async fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let cfg = {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().await;
        cfg.streamdeck_enabled = enabled;
        cfg.clone()
    };
    config::save(&cfg)?;
    app.state::<StreamDeck>().switch(app, enabled);
    let _ = app.emit(STATUS_EVENT, ());
    Ok(())
}
