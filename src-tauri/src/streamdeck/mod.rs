//! Stream Deck: the local channel that the Deskmate plugin for Stream Deck
//! talks to.
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

mod action_ids;
mod app;
mod backend;
mod entities;
mod framing;
mod pipe_name;
#[cfg(windows)]
mod pipe_security;
mod protocol;
#[cfg(windows)]
mod server;
mod session;
mod watch;

#[cfg(all(test, windows))]
mod server_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod stand_ins;

use tauri::AppHandle;

pub use app::{ha_settings_changed, init, StreamDeckView};

/// Settings > Stream Deck: the switch and the channel.
#[tauri::command]
pub async fn streamdeck_status(app: AppHandle) -> Result<StreamDeckView, String> {
    app::view(&app).await
}

/// Opens or closes the channel and stores the switch. Takes effect at once.
#[tauri::command]
pub async fn set_streamdeck_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    app::set_enabled(&app, enabled).await
}
