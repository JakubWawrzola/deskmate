//! Deskmate - Windows companion for Home Assistant.
//! Composition root: state, tauri commands, tray, window.

mod actions;
mod clipboard;
mod config;
mod consts;
mod discovery;
mod ha_api;
mod hotkeys;
mod media;
mod mqtt;
mod notify;
mod sensors;
mod security;
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
    /// whether the HA API channel (URL + token) is configured
    ha_configured: bool,
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
    if !matches!(new_config.mqtt_transport.as_str(), "tls" | "insecure") {
        return Err("MQTT transport must be tls or insecure".into());
    }
    if new_config.mqtt_transport == "insecure" && !new_config.broker_host_remote.trim().is_empty() {
        return Err("MQTT fallback address requires TLS; plaintext MQTT is trusted-LAN only".into());
    }
    if !matches!(new_config.clipboard_read_mode.as_str(), "off" | "confirm" | "automatic")
        || !matches!(new_config.clipboard_write_mode.as_str(), "off" | "confirm" | "automatic")
    {
        return Err("clipboard mode must be off, confirm or automatic".into());
    }
    if new_config.broker_port == 0 {
        return Err("MQTT broker port must be between 1 and 65535".into());
    }
    new_config.publish_interval_secs = new_config.publish_interval_secs.clamp(2, 3600);
    let mut custom_ids = std::collections::HashSet::new();
    for command in &mut new_config.custom_commands {
        let normalized_id = config::sanitize_id(&command.id);
        if normalized_id != command.id || command.id.is_empty() || !custom_ids.insert(command.id.clone()) {
            return Err(format!("invalid or duplicate custom command id: {}", command.id));
        }
        command.name = command.name.trim().chars().take(120).collect();
        if command.name.is_empty() || command.command.trim().is_empty() || command.command.len() > 8192 {
            return Err(format!("invalid custom command: {}", command.id));
        }
        if !matches!(command.kind.as_str(), "button" | "switch" | "number")
            || !command.num_min.is_finite()
            || !command.num_max.is_finite()
            || !command.num_step.is_finite()
            || command.num_step <= 0.0
            || command.num_min > command.num_max
        {
            return Err(format!("invalid custom command settings: {}", command.id));
        }
    }
    let mut origins = Vec::new();
    for origin in &new_config.allowed_url_origins {
        let normalized = sys_commands::normalize_allowed_origin(origin)
            .map_err(|e| format!("invalid allowed URL origin '{origin}': {e}"))?;
        if !origins.contains(&normalized) {
            origins.push(normalized);
        }
    }
    new_config.allowed_url_origins = origins;
    // clean up an orphaned Credential Manager entry when user/host changes
    {
        let old = state.config.lock().await.clone();
        if old.username != new_config.username || old.broker_host != new_config.broker_host {
            config::delete_password_for(&old.username, &old.broker_host);
        }
    }
    if let Some(pw) = password {
        config::set_password(&new_config, &pw)?;
    }
    let branding = new_config.toast_branding;
    config::save(&new_config)?;
    *state.config.lock().await = new_config;
    // branding in the background (Start Menu shortcut) + restart the connection with the new config
    std::thread::spawn(move || notify::apply_branding(branding));
    tauri::async_runtime::spawn(mqtt::restart(app));
    Ok(())
}

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> Result<Snapshot, String> {
    let cfg = state.config.lock().await.clone();
    Ok(Snapshot {
        status: state.status.lock().await.clone(),
        sensor_values: state.sensor_values.lock().await.clone(),
        published_count: state.published_count.load(Ordering::Relaxed),
        notifications: state.notif_history.lock().await.iter().cloned().collect(),
        sensor_defs: sensors::SENSOR_DEFS.to_vec(),
        command_defs: sys_commands::COMMAND_DEFS.to_vec(),
        hostname: config::hostname(),
        ha_configured: ha_api::is_configured(&cfg),
    })
}

// ---------- Home Assistant API (F1) ----------

/// Saves the HA URLs + token (token -> Credential Manager, not config.json).
/// Empty token = keep the existing one; token "-" = delete it.
#[tauri::command]
async fn set_ha_api(
    state: State<'_, AppState>,
    url: String,
    url_remote: String,
    token: Option<String>,
) -> Result<(), String> {
    let url = ha_api::normalize_base_url(&url)?;
    let url_remote = ha_api::require_https_base_url(&url_remote)?;
    let cfg = {
        let mut cfg = state.config.lock().await;
        cfg.ha_url = url;
        cfg.ha_url_remote = url_remote;
        cfg.clone()
    };
    config::save(&cfg)?;
    if let Some(t) = token {
        let t = t.trim().to_string();
        if t == "-" {
            ha_api::set_token(&cfg, "")?;
        } else if !t.is_empty() {
            ha_api::set_token(&cfg, &t)?;
        }
    }
    Ok(())
}

/// Tests the connection to HA (uses the current config + token).
#[tauri::command]
async fn ha_ping(state: State<'_, AppState>) -> Result<String, String> {
    let cfg = state.config.lock().await.clone();
    tokio::task::spawn_blocking(move || ha_api::ping(&cfg))
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
}

// ---------- Hotkeys (F2) ----------

/// Replaces the hotkey list: save + re-register + device trigger discovery.
/// Returns a list of registration errors (shortcut already taken, etc.) - the rest still works.
#[tauri::command]
async fn update_hotkeys(
    app: AppHandle,
    state: State<'_, AppState>,
    hotkeys: Vec<config::Hotkey>,
) -> Result<Vec<String>, String> {
    // sanitize ids + validate accelerators before saving
    let mut clean = Vec::new();
    for mut h in hotkeys {
        h.id = config::sanitize_id(&h.id);
        if h.id.is_empty() {
            return Err("hotkey id is required".into());
        }
        if !h.accelerator.trim().is_empty() {
            hotkeys::parse(&h.accelerator)?;
        }
        clean.push(h);
    }
    let (cfg, removed) = {
        let mut cfg = state.config.lock().await;
        let old_ids: Vec<String> = cfg.hotkeys.iter().map(|h| h.id.clone()).collect();
        cfg.hotkeys = clean;
        let new_ids: Vec<String> = cfg.hotkeys.iter().map(|h| h.id.clone()).collect();
        let removed: Vec<String> = old_ids.into_iter().filter(|i| !new_ids.contains(i)).collect();
        (cfg.clone(), removed)
    };
    config::save(&cfg)?;
    let errors = hotkeys::register_all(&app).await;
    // discovery: new/changed triggers + clean up removed ones
    let client = state.client.lock().await.clone();
    if let Some(client) = client {
        for (topic, payload) in discovery::build_all(&cfg) {
            let _ = client.publish(topic, rumqttc::QoS::AtLeastOnce, true, payload).await;
        }
        for id in removed {
            let (topic, payload) = discovery::remove_hotkey(&cfg.node_id, &id);
            let _ = client.publish(topic, rumqttc::QoS::AtLeastOnce, true, payload).await;
        }
    }
    Ok(errors)
}

// ---------- Widgets (F3) ----------

#[tauri::command]
async fn update_widgets(
    state: State<'_, AppState>,
    widgets: Vec<config::WidgetItem>,
) -> Result<(), String> {
    let cfg = {
        let mut cfg = state.config.lock().await;
        cfg.widgets = widgets
            .into_iter()
            .filter(|w| !w.entity_id.trim().is_empty())
            .collect();
        cfg.clone()
    };
    config::save(&cfg)
}

#[derive(serde::Serialize)]
struct WidgetState {
    entity_id: String,
    label: String,
    state: String,
    /// whether the entity can be toggled by a click (toggleable domain)
    togglable: bool,
}

/// Widget panel tile states (polled from the widget window every ~3 s).
#[tauri::command]
async fn widget_states(state: State<'_, AppState>) -> Result<Vec<WidgetState>, String> {
    let cfg = state.config.lock().await.clone();
    if !ha_api::is_configured(&cfg) {
        return Err("HA API not configured".into());
    }
    let items = cfg.widgets.clone();
    let ids: Vec<String> = items.iter().map(|w| w.entity_id.trim().to_string()).collect();
    let cfg2 = cfg.clone();
    let states = tokio::task::spawn_blocking(move || ha_api::get_states_for(&cfg2, &ids))
        .await
        .map_err(|e| e.to_string())?;
    const TOGGLABLE: &[&str] = &["light", "switch", "fan", "input_boolean", "media_player", "cover", "script", "scene", "automation", "humidifier", "siren"];
    let out = items
        .iter()
        .map(|w| {
            let id = w.entity_id.trim();
            let found = states.iter().find(|s| s.entity_id == id);
            let friendly = found
                .and_then(|s| s.attributes.get("friendly_name"))
                .and_then(|v| v.as_str())
                .unwrap_or(id)
                .to_string();
            let domain = id.split('.').next().unwrap_or("");
            WidgetState {
                entity_id: id.to_string(),
                label: if w.label.trim().is_empty() { friendly } else { w.label.clone() },
                state: found.map(|s| s.state.clone()).unwrap_or_else(|| "unavailable".into()),
                togglable: TOGGLABLE.contains(&domain),
            }
        })
        .collect();
    Ok(out)
}

/// Tile click: toggle the entity (scene/script -> turn_on).
#[tauri::command]
async fn widget_toggle(state: State<'_, AppState>, entity_id: String) -> Result<(), String> {
    let cfg = state.config.lock().await.clone();
    tokio::task::spawn_blocking(move || {
        let domain = entity_id.split('.').next().unwrap_or("");
        match domain {
            "scene" | "script" => ha_api::call_service(&cfg, domain, "turn_on", Some(&entity_id), &serde_json::json!({})),
            _ => ha_api::toggle(&cfg, &entity_id),
        }
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()))
}

#[tauri::command]
fn toggle_widget_window(app: AppHandle) {
    actions::toggle_widget_window(&app);
}

// ---------- Tray quick actions (F4) ----------

#[tauri::command]
async fn update_tray_actions(
    app: AppHandle,
    state: State<'_, AppState>,
    tray_actions: Vec<config::TrayAction>,
) -> Result<(), String> {
    let cfg = {
        let mut cfg = state.config.lock().await;
        cfg.tray_actions = tray_actions
            .into_iter()
            .map(|mut t| {
                t.id = config::sanitize_id(&t.id);
                t
            })
            .filter(|t| !t.id.is_empty() && !t.name.trim().is_empty())
            .collect();
        cfg.clone()
    };
    config::save(&cfg)?;
    rebuild_tray_menu(&app, &cfg)?;
    Ok(())
}

/// Builds the tray menu: quick actions + Widgets + Open/Quit.
fn rebuild_tray_menu(app: &AppHandle, cfg: &AppConfig) -> Result<(), String> {
    use tauri::menu::PredefinedMenuItem;
    let mk = |id: &str, label: &str| MenuItem::with_id(app, id, label, true, None::<&str>);
    let mut items: Vec<tauri::menu::MenuItem<tauri::Wry>> = Vec::new();
    for t in &cfg.tray_actions {
        items.push(mk(&format!("qa_{}", t.id), &t.name).map_err(|e| e.to_string())?);
    }
    let widgets = mk("widgets", "Show/hide widgets").map_err(|e| e.to_string())?;
    let open = mk("open", "Open Deskmate").map_err(|e| e.to_string())?;
    let quit = mk("quit", "Quit").map_err(|e| e.to_string())?;
    let sep = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let mut refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = Vec::new();
    for it in &items {
        refs.push(it);
    }
    if !items.is_empty() {
        refs.push(&sep);
    }
    refs.push(&widgets);
    refs.push(&open);
    refs.push(&quit);
    let menu = Menu::with_items(app, &refs).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    }
    Ok(())
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
    // republish discovery (add/remove entity in HA) without a full restart
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

/// Immediate toggle of an opt-in option (like sensors) - without waiting for Save.
/// allow_input/tts_enabled: save + republish discovery (entities appear right away).
/// toast_branding: save + rebuild branding in the background.
#[tauri::command]
async fn set_feature_flag(
    state: State<'_, AppState>,
    flag: String,
    enabled: bool,
) -> Result<(), String> {
    let cfg = {
        let mut cfg = state.config.lock().await;
        match flag.as_str() {
            "allow_input" => cfg.allow_input = enabled,
            "tts_enabled" => cfg.tts_enabled = enabled,
            "toast_branding" => cfg.toast_branding = enabled,
            _ => return Err(format!("unknown flag: {flag}")),
        }
        cfg.clone()
    };
    config::save(&cfg)?;
    if flag == "toast_branding" {
        std::thread::spawn(move || notify::apply_branding(enabled));
    } else {
        // republish discovery without a restart (text TTS/type_text + presentation entities)
        let client = state.client.lock().await.clone();
        if let Some(client) = client {
            for (topic, payload) in discovery::build_all(&cfg) {
                let _ = client
                    .publish(topic, rumqttc::QoS::AtLeastOnce, true, payload)
                    .await;
            }
        }
    }
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
            enabled: false,
            require_confirmation: true,
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
async fn update_custom_command_security(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    require_confirmation: bool,
) -> Result<(), String> {
    let cfg = {
        let mut cfg = state.config.lock().await;
        let command = cfg
            .custom_commands
            .iter_mut()
            .find(|command| command.id == id)
            .ok_or_else(|| format!("unknown command: {id}"))?;
        command.enabled = enabled;
        command.require_confirmation = require_confirmation;
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
fn test_toast(state: State<'_, AppState>) -> Result<(), String> {
    // example with buttons: a click publishes to deskmate/<node>/notify/action
    notify::show_toast(
        &notify::NotifyPayload {
            title: consts::TOAST_DISPLAY_NAME.into(),
            message: "Notifications work. Try the buttons below.".into(),
            image: None,
            actions: vec![
                notify::NotifyAction { title: "OK".into(), action: "test_ok".into() },
                notify::NotifyAction { title: "Snooze".into(), action: "test_snooze".into() },
            ],
        },
        Some(state.action_tx.clone()),
    )
}

#[tauri::command]
fn run_command_local(state: State<'_, AppState>, id: String) -> Result<(), String> {
    // test a command from the UI (same path as MQTT)
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
    let toast_branding = cfg.toast_branding;

    tauri::Builder::default()
        // single-instance MUST be first: when a toast button click launches
        // deskmate:action?name=..., this process forwards the URL to the running app
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            for arg in &argv {
                if let Some(action) = notify::parse_action_url(arg) {
                    let _ = app.state::<AppState>().action_tx.send(action);
                }
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // global hotkeys: a single handler, dispatch based on config (hotkeys.rs)
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        hotkeys::on_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .manage(AppState::new(cfg))
        .setup(move |app| {
            // Toast branding (Start Menu shortcut with AUMID) in the background - doesn't block startup.
            // If it fails or is disabled, show_toast falls back to PowerShell AUMID.
            std::thread::spawn(move || notify::apply_branding(toast_branding));

            // deskmate scheme: for toast buttons (click -> deskmate:action?name=...)
            notify::register_protocol();
            // when the app was launched directly from a protocol URL (wasn't already running)
            {
                let st = app.state::<AppState>();
                for arg in std::env::args() {
                    if let Some(action) = notify::parse_action_url(&arg) {
                        let _ = st.action_tx.send(action);
                    }
                }
            }

            // drain toast actions (button click) -> publish to MQTT
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

            // tray: quick actions (configurable) + widgets + Open/Quit;
            // closing the window = hiding to the tray
            let open = MenuItem::with_id(app, "open", "Open Deskmate", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Deskmate")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    let id = event.id.as_ref().to_string();
                    match id.as_str() {
                        "open" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "widgets" => actions::toggle_widget_window(app),
                        "quit" => app.exit(0),
                        _ => {
                            // quick action: qa_<id> -> execute the ActionSpec from config
                            if let Some(qa_id) = id.strip_prefix("qa_").map(|s| s.to_string()) {
                                let app = app.clone();
                                tauri::async_runtime::spawn(async move {
                                    let spec = {
                                        let st = app.state::<AppState>();
                                        let cfg = st.config.lock().await;
                                        cfg.tray_actions.iter().find(|t| t.id == qa_id).map(|t| t.action.clone())
                                    };
                                    if let Some(spec) = spec {
                                        actions::execute(&app, &spec, &qa_id).await;
                                    }
                                });
                            }
                        }
                    }
                })
                .build(app)?;

            // tray menu with quick actions from config + hotkey registration
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let cfg = handle.state::<AppState>().config.lock().await.clone();
                    let _ = rebuild_tray_menu(&handle, &cfg);
                    let _ = hotkeys::register_all(&handle).await;
                });
            }

            if launch_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            // start MQTT if configured
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(mqtt::restart(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // closing = minimize to the tray (app keeps running in the background)
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_snapshot,
            set_sensor_enabled,
            set_feature_flag,
            add_custom_command,
            update_custom_command_security,
            remove_custom_command,
            restart_connection,
            test_toast,
            run_command_local,
            set_ha_api,
            ha_ping,
            update_hotkeys,
            update_widgets,
            widget_states,
            widget_toggle,
            toggle_widget_window,
            update_tray_actions
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
