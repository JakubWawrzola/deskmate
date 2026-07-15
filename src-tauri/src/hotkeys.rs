//! Global keyboard shortcuts (tauri-plugin-global-shortcut).
//! There is a single handler (in the plugin's Builder); this module handles
//! registration/normalization. Shortcuts work system-wide - even when Deskmate
//! is sitting in the tray.

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// Normalizes an accelerator typed by the user into the parser's format
/// (letter -> KeyX, digit -> DigitN; modifiers left unchanged).
/// "Ctrl+Alt+L" -> "Ctrl+Alt+KeyL", "Ctrl+Shift+2" -> "Ctrl+Shift+Digit2".
pub fn normalize(accel: &str) -> String {
    let parts: Vec<&str> = accel.split('+').map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return String::new();
    }
    let (mods, key) = parts.split_at(parts.len() - 1);
    let k = key[0];
    let norm_key = if k.len() == 1 {
        let c = k.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            format!("Key{}", c.to_ascii_uppercase())
        } else if c.is_ascii_digit() {
            format!("Digit{c}")
        } else {
            k.to_string()
        }
    } else {
        k.to_string()
    };
    let mut out: Vec<String> = mods.iter().map(|m| m.to_string()).collect();
    out.push(norm_key);
    out.join("+")
}

/// Parses a user accelerator into a Shortcut (after normalization).
pub fn parse(accel: &str) -> Result<Shortcut, String> {
    normalize(accel)
        .parse::<Shortcut>()
        .map_err(|e| format!("invalid shortcut '{accel}': {e}"))
}

/// (Re)registers all hotkeys from the config. Returns a list of errors
/// (e.g. shortcut already taken by another application) - the rest keep working.
pub async fn register_all(app: &AppHandle) -> Vec<String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let cfg = app.state::<crate::state::AppState>().config.lock().await.clone();
    let mut errors = Vec::new();
    for h in &cfg.hotkeys {
        if h.accelerator.trim().is_empty() {
            continue;
        }
        match parse(&h.accelerator) {
            Ok(sc) => {
                if let Err(e) = gs.register(sc) {
                    errors.push(format!("{} ({}): {}", h.name, h.accelerator, e));
                }
            }
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        log::warn!("hotkey registration issues: {errors:?}");
    }
    errors
}

/// Handler for a pressed shortcut: finds the matching hotkey in the config and runs its action.
/// Called from the plugin (event thread) - the work is offloaded to the async runtime.
pub fn on_shortcut(app: &AppHandle, pressed: &Shortcut) {
    let app = app.clone();
    let pressed = *pressed;
    tauri::async_runtime::spawn(async move {
        let cfg = app
            .state::<crate::state::AppState>()
            .config
            .lock()
            .await
            .clone();
        for h in &cfg.hotkeys {
            if let Ok(sc) = parse(&h.accelerator) {
                if sc == pressed {
                    log::info!("hotkey fired: {} ({})", h.id, h.accelerator);
                    crate::actions::execute(&app, &h.action, &h.id).await;
                    return;
                }
            }
        }
    });
}
