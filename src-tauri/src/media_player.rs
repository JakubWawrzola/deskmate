//! The Link media player (`deskmate_core::link::media`) in the app: reads the
//! media session, the volume and the mute state, sends the state object when
//! any part of it changes, and runs Home Assistant's commands with the same
//! media and volume actions as the separate media buttons and controls.

use deskmate_core::link::media::{
    declares_media_player, media_state_frame, parse_media_command, MediaOp, MediaPlayerState,
    MediaSession,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use crate::config::AppConfig;
use crate::state::AppState;

/// The state object last sent in this session. Cleared when a send fails, so
/// the next session gets the state again.
static LAST_SENT: Mutex<Option<Value>> = Mutex::new(None);

/// Reads what the media player shows. Blocking: the media session and the
/// sound device are COM and WinRT calls.
fn read(cfg: &AppConfig) -> MediaPlayerState {
    let session = crate::media::current().map(|info| MediaSession {
        status: info.status,
        title: info.title,
        artist: info.artist,
        app: info.app,
    });
    MediaPlayerState::new(
        cfg,
        session.as_ref(),
        crate::sys_commands::get_volume(),
        crate::sys_commands::audio_muted(),
    )
}

/// Sends the state object of the media player when it differs from the one
/// sent last, or in any case with `force`. Does nothing while the current
/// Link session does not declare the media player.
pub async fn publish(app: &AppHandle, force: bool) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    if cfg.transport != "link" || !declares_media_player(&cfg, state.link_info().as_ref()) {
        return;
    }
    let reading = match tokio::task::spawn_blocking(move || read(&cfg)).await {
        Ok(reading) => reading,
        Err(error) => {
            log::warn!("reading the media player failed: {error}");
            return;
        }
    };
    let value = reading.to_value();
    {
        let mut last = LAST_SENT.lock().unwrap_or_else(|e| e.into_inner());
        if !force && last.as_ref() == Some(&value) {
            return;
        }
        *last = Some(value);
    }
    if !crate::link::send(app, media_state_frame(&reading)).await {
        *LAST_SENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Runs a command for the media player. Refused while the current session
/// does not declare it, which also covers MQTT.
pub async fn handle_command(
    app: &AppHandle,
    action: &str,
    value: Option<&Value>,
) -> Result<(), String> {
    let declared = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().await;
        declares_media_player(&cfg, state.link_info().as_ref())
    };
    if !declared {
        return Err("the media player is not enabled on this computer".into());
    }
    let op = parse_media_command(action, value)?;
    tokio::task::spawn_blocking(move || run(op))
        .await
        .map_err(|e| e.to_string())??;
    // The volume control and the mute switch show the same values.
    let echoed = match op {
        MediaOp::SetVolume(_) => tokio::task::spawn_blocking(crate::sys_commands::get_volume)
            .await
            .ok()
            .flatten()
            .map(|volume| ("volume", volume.to_string())),
        MediaOp::Mute(muted) => Some(("audio_mute", if muted { "ON" } else { "OFF" }.into())),
        _ => None,
    };
    if let Some((key, value)) = echoed {
        let values = HashMap::from([(key.to_string(), value)]);
        crate::transport::publish_states(app, &values).await;
    }
    publish(app, false).await;
    Ok(())
}

/// The existing actions behind each op. The media session offers play and
/// pause of their own, so neither goes through the toggle.
fn run(op: MediaOp) -> Result<(), String> {
    match op {
        MediaOp::PlayPause => crate::media::control("play_pause"),
        MediaOp::Play => crate::media::control("play"),
        MediaOp::Pause => crate::media::control("pause"),
        MediaOp::Next => crate::media::control("next"),
        MediaOp::Previous => crate::media::control("prev"),
        MediaOp::SetVolume(volume) => {
            crate::sys_commands::run_builtin("volume", &(volume * 100.0).to_string())
        }
        MediaOp::Mute(muted) => crate::sys_commands::set_audio_muted(muted),
    }
}
