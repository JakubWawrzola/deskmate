//! Wspolny wykonawca ActionSpec (hotkeye, tray quick actions).
//! kind: toggle | service | command | widget | mqtt

use tauri::{AppHandle, Manager};

use crate::config::ActionSpec;
use crate::state::AppState;

/// Wykonuje akcje. source_id = id hotkeya/pozycji tray (do topicu mqtt i logow).
pub async fn execute(app: &AppHandle, spec: &ActionSpec, source_id: &str) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    match spec.kind.as_str() {
        "toggle" => {
            let entity = spec.entity_id.trim().to_string();
            if entity.is_empty() {
                log::warn!("action {source_id}: toggle without entity_id");
                return;
            }
            let r = tokio::task::spawn_blocking(move || crate::ha_api::toggle(&cfg, &entity))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            if let Err(e) = r {
                log::warn!("action {source_id}: {e}");
            }
        }
        "service" => {
            let Some((domain, service)) = spec.service.trim().split_once('.') else {
                log::warn!("action {source_id}: service must be domain.service");
                return;
            };
            let data: serde_json::Value = if spec.data.trim().is_empty() {
                serde_json::json!({})
            } else {
                match serde_json::from_str(&spec.data) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!("action {source_id}: bad data JSON: {e}");
                        return;
                    }
                }
            };
            let (d, s) = (domain.to_string(), service.to_string());
            let entity = spec.entity_id.trim().to_string();
            let r = tokio::task::spawn_blocking(move || {
                let ent = if entity.is_empty() { None } else { Some(entity.as_str()) };
                crate::ha_api::call_service(&cfg, &d, &s, ent, &data)
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            if let Err(e) = r {
                log::warn!("action {source_id}: {e}");
            }
        }
        "command" => {
            let id = spec.command_id.clone();
            let r = tokio::task::spawn_blocking(move || {
                if let Some(cid) = id.strip_prefix("custom_") {
                    crate::sys_commands::run_custom(&cfg, cid, "")
                } else {
                    crate::sys_commands::run_builtin(&id, "")
                }
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            if let Err(e) = r {
                log::warn!("action {source_id}: {e}");
            }
        }
        "widget" => toggle_widget_window(app),
        "mqtt" => {
            // Event to HA over the selected transport; no HA API token needed.
            crate::transport::publish_trigger(app, source_id).await;
        }
        other => log::warn!("action {source_id}: unknown kind '{other}'"),
    }
}

/// Pokaz/schowaj panel widgetow (okno "widget").
pub fn toggle_widget_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("widget") {
        let visible = w.is_visible().unwrap_or(false);
        if visible {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}
