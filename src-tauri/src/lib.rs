//! Deskmate - Windows companion for Home Assistant.
//! Punkt skladania: stan, komendy tauri, tray, okno.

mod clipboard;
mod config;
mod consts;
mod discovery;
mod media;
mod mqtt;
mod notify;
mod sensors;
mod state;
mod sys_commands;
mod tts;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State};

use config::AppConfig;
use state::AppState;

#[derive(serde::Serialize)]
struct ConfigView {
    config: AppConfig,
    has_password: bool,
}

#[derive(serde::Serialize)]
struct Snapshot {
    status: state::StatusView,
    sensor_values: HashMap<String, String>,
    published_count: u64,
    notifications: Vec<notify::NotifyRecord>,
    sensor_defs: Vec<sensors::SensorDef>,
    command_defs: Vec<sys_commands::CommandDef>,
    hostname: String,
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<ConfigView, String> {
    let cfg = state.config.lock().await.clone();
    let has_password = config::get_password(&cfg).is_some();
    Ok(ConfigView { config: cfg, has_password })
}

#[tauri::command]
async fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    mut new_config: AppConfig,
    password: Option<String>,
) -> Result<(), String> {
    new_config.node_id = config::sanitize_id(if new_config.node_id.is_empty() {
        &new_config.device_name
    } else {
        &new_config.node_id
    });
    new_config.configured = !new_config.broker_host.is_empty();
    // sprzatanie osieroconego wpisu w Credential Manager przy zmianie user/host
    {
        let old = state.config.lock().await.clone();
        if old.username != new_config.username || old.broker_host != new_config.broker_host {
            config::delete_password_for(&old.username, &old.broker_host);
        }
    }
    if let Some(pw) = password {
        config::set_password(&new_config, &pw)?;
    }
    config::save(&new_config)?;
    *state.config.lock().await = new_config;
    // restart polaczenia z nowym configiem
    tauri::async_runtime::spawn(mqtt::restart(app));
    Ok(())
}

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> Result<Snapshot, String> {
    Ok(Snapshot {
        status: state.status.lock().await.clone(),
        sensor_values: state.sensor_values.lock().await.clone(),
        published_count: state.published_count.load(Ordering::Relaxed),
        notifications: state.notif_history.lock().await.iter().cloned().collect(),
        sensor_defs: sensors::SENSOR_DEFS.to_vec(),
        command_defs: sys_commands::COMMAND_DEFS.to_vec(),
        hostname: config::hostname(),
    })
}

#[tauri::command]
async fn set_sensor_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let cfg = {
        let mut cfg = state.config.lock().await;
        cfg.sensors_enabled.insert(id, enabled);
        cfg.clone()
    };
    config::save(&cfg)?;
    // republikacja discovery (dodanie/usuniecie encji w HA) bez pelnego restartu
    let client = state.client.lock().await.clone();
    if let Some(client) = client {
        for (topic, payload) in discovery::build_all(&cfg) {
            let _ = client
                .publish(topic, rumqttc::QoS::AtLeastOnce, true, payload)
                .await;
        }
    }
    let _ = app;
    Ok(())
}

#[tauri::command]
async fn add_custom_command(
    state: State<'_, AppState>,
    id: String,
    name: String,
    command: String,
    kind: Option<String>,
    num_min: Option<f64>,
    num_max: Option<f64>,
    num_step: Option<f64>,
) -> Result<(), String> {
    let id_s = config::sanitize_id(&id);
    if id_s.is_empty() || name.trim().is_empty() || command.trim().is_empty() {
        return Err("id, name and command are required".into());
    }
    let kind = kind.unwrap_or_else(|| "button".into());
    if !["button", "switch", "number"].contains(&kind.as_str()) {
        return Err("kind must be button|switch|number".into());
    }
    let cfg = {
        let mut cfg = state.config.lock().await;
        if cfg.custom_commands.iter().any(|c| c.id == id_s) {
            return Err(format!("command '{id_s}' already exists"));
        }
        cfg.custom_commands.push(config::CustomCommand {
            id: id_s,
            name: name.trim().into(),
            command: command.trim().into(),
            kind,
            num_min: num_min.unwrap_or(0.0),
            num_max: num_max.unwrap_or(100.0),
            num_step: num_step.unwrap_or(1.0),
        });
        cfg.clone()
    };
    config::save(&cfg)?;
    let client = state.client.lock().await.clone();
    if let Some(client) = client {
        for (topic, payload) in discovery::build_all(&cfg) {
            let _ = client
                .publish(topic, rumqttc::QoS::AtLeastOnce, true, payload)
                .await;
        }
    }
    Ok(())
}

#[tauri::command]
async fn remove_custom_command(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let cfg = {
        let mut cfg = state.config.lock().await;
        cfg.custom_commands.retain(|c| c.id != id);
        cfg.clone()
    };
    config::save(&cfg)?;
    let client = state.client.lock().await.clone();
    if let Some(client) = client {
        for (topic, payload) in discovery::remove_custom(&cfg.node_id, &id) {
            let _ = client
                .publish(topic, rumqttc::QoS::AtLeastOnce, true, payload)
                .await;
        }
    }
    Ok(())
}

#[tauri::command]
async fn restart_connection(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn(mqtt::restart(app));
    Ok(())
}

#[tauri::command]
fn test_toast() -> Result<(), String> {
    notify::show_toast(
        &notify::NotifyPayload {
            title: "Deskmate".into(),
            message: "Notifications are working.".into(),
            image: None,
            actions: Vec::new(),
        },
        None,
    )
}

#[tauri::command]
fn run_command_local(state: State<'_, AppState>, id: String) -> Result<(), String> {
    // test komendy z UI (ta sama sciezka co MQTT)
    if let Some(cid) = id.strip_prefix("custom_") {
        let cfg = state.config.blocking_lock().clone();
        sys_commands::run_custom(&cfg, cid, "")
    } else {
        sys_commands::run_builtin(&id, "")
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cfg = config::load();
    let launch_hidden = cfg.launch_hidden;

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState::new(cfg))
        .setup(move |app| {
            // AUMID: wpis HKCU + skrot w Menu Start z AppUserModelID (branding toastow;
            // gdy skrot sie nie uda, show_toast spada na PowerShell AUMID)
            notify::ensure_aumid_registered();
            notify::ensure_branding();

            // drain akcji z toastow (klik przycisku) -> publikacja na MQTT
            {
                let handle = app.handle().clone();
                let rx = app
                    .state::<AppState>()
                    .action_rx
                    .try_lock()
                    .ok()
                    .and_then(|mut g| g.take());
                if let Some(mut rx) = rx {
                    tauri::async_runtime::spawn(async move {
                        while let Some(action) = rx.recv().await {
                            mqtt::publish_action(&handle, &action).await;
                        }
                    });
                }
            }

            // tray: Open + Quit; zamkniecie okna = chowanie do traya
            let open = MenuItem::with_id(app, "open", "Open Deskmate", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Deskmate")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            if launch_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            // start MQTT jesli skonfigurowane
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(mqtt::restart(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // zamkniecie = minimalizacja do traya (app dziala w tle)
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_snapshot,
            set_sensor_enabled,
            add_custom_command,
            remove_custom_command,
            restart_connection,
            test_toast,
            run_command_local
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
