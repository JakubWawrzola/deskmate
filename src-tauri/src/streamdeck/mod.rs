//! Stream Deck: the local channel that the Deskmate plugin for Stream Deck
//! talks to, and the installer of that plugin.
//!
//! The channel is a named pipe of the current Windows user, specified in the
//! "Local channel" section of `docs/STREAMDECK.md`; what it exposes and to
//! whom is in `docs/SECURITY.md`. It exists only while the user has Stream
//! Deck switched on in Settings.
//!
//! - `pipe_name`, `pipe_security`, `server`: the pipe, its access rules, its
//!   instances and connections.
//! - `framing`, `protocol`, `session`: one connection, from bytes to
//!   answered requests.
//! - `watch`, `entities`, `action_ids`: what the requests work on.
//! - `backend`: what a connection needs from the app, as traits.
//! - `app`: those traits on top of `ha_api` and `actions`, and the state.
//! - `plugin_install`: copying the plugin into Stream Deck's folder.

mod action_ids;
mod app;
mod backend;
mod entities;
mod framing;
mod pipe_name;
#[cfg(windows)]
mod pipe_security;
mod plugin_install;
mod protocol;
#[cfg(windows)]
mod server;
mod session;
mod watch;

#[cfg(test)]
mod plugin_install_tests;
#[cfg(all(test, windows))]
mod server_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod stand_ins;

use tauri::AppHandle;

pub use app::{ha_settings_changed, init, StreamDeckView};

/// Settings > Stream Deck: the switch, the channel and the plugin.
#[tauri::command]
pub async fn streamdeck_status(app: AppHandle) -> Result<StreamDeckView, String> {
    app::view(&app).await
}

/// Opens or closes the channel and stores the switch. Takes effect at once.
#[tauri::command]
pub async fn set_streamdeck_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    app::set_enabled(&app, enabled).await
}

/// Copies the plugin that ships with the app into Stream Deck's plugin
/// folder. The paths are the app's own; the window supplies nothing.
#[tauri::command]
pub async fn install_streamdeck_plugin(app: AppHandle) -> Result<(), String> {
    app::install_plugin(&app).await
}

/// Deletes the plugin from Stream Deck's plugin folder.
#[tauri::command]
pub async fn remove_streamdeck_plugin(app: AppHandle) -> Result<(), String> {
    app::remove_plugin(&app).await
}
