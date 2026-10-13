//! "Start with Windows" kept across updates (GitHub issue #7).
//!
//! Running a newer installer over the old version first runs the old
//! uninstaller, and Tauri's uninstaller deletes the Run entry unless it was
//! started as a silent update. So every manual update switched autostart off.
//! The choice now also lives in config.json, and a missing Run entry is put
//! back on the next start. An entry the user switched off in Task Manager
//! (StartupApproved) still exists and is left alone.

use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::state::AppState;

/// HKCU Run value written by tauri-plugin-autostart: the product name.
#[cfg(windows)]
const RUN_VALUE: &str = "Deskmate";

#[cfg(windows)]
fn run_entry_exists() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .and_then(|key| key.get_raw_value(RUN_VALUE))
        .is_ok()
}

#[cfg(not(windows))]
fn run_entry_exists() -> bool {
    true
}

/// At startup: restore a Run entry an update removed, or record the current
/// state once for configs written before the setting was remembered.
pub fn reconcile(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().await;
        match cfg.start_with_windows {
            Some(true) if !run_entry_exists() => match app.autolaunch().enable() {
                Ok(()) => log::info!("start with Windows restored after an update"),
                Err(e) => log::warn!("cannot restore start with Windows: {e}"),
            },
            None => {
                cfg.start_with_windows = Some(run_entry_exists());
                let _ = crate::config::save(&cfg);
            }
            _ => {}
        }
    });
}

#[tauri::command]
pub fn get_start_with_windows(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub async fn set_start_with_windows(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(|e| e.to_string())?;
    } else {
        launcher.disable().map_err(|e| e.to_string())?;
    }
    let state = app.state::<AppState>();
    let mut cfg = state.config.lock().await;
    cfg.start_with_windows = Some(enabled);
    crate::config::save(&cfg)
}
