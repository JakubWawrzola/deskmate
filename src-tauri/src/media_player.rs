//! The Link media player (`deskmate_core::link::media`) in the app: reads the
//! media session, the volume and the mute state, sends the state object when
//! any part of it changes, and runs Home Assistant's commands with the same
//! media and volume actions as the separate media buttons and controls.

use deskmate_core::link::media::{
    art_track, declares_media_player, media_art_frame, media_state_frame, parse_media_command,
    takes_media_art, MediaOp, MediaPlayerState, MediaSession, VOLUME_SENSOR,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use crate::config::AppConfig;
use crate::media_art::{ArtStep, ArtTracker};
use crate::state::AppState;

/// The state last sent in this session. Cleared when a send fails, so the
/// next session gets the state again.
static LAST_SENT: Mutex<Option<MediaPlayerState>> = Mutex::new(None);

/// Which cover Home Assistant has, so that each one is sent once.
static ART: Mutex<ArtTracker> = Mutex::new(ArtTracker::new());

/// Reads what the media player shows, and the track a cover would belong
/// to. Blocking: the media session and the sound device are COM and WinRT
/// calls.
fn read(cfg: &AppConfig) -> (MediaPlayerState, Option<String>) {
    let session = crate::media::current().map(|info| MediaSession {
        status: info.status,
        title: info.title,
        artist: info.artist,
        album: info.album,
        app: info.app,
        timeline: info.timeline,
    });
    let state = MediaPlayerState::new(
        cfg,
        session.as_ref(),
        crate::sys_commands::get_volume(),
        crate::sys_commands::audio_muted(),
    );
    (state, art_track(cfg, session.as_ref()))
}

/// Sends the state object of the media player when it differs from the one
/// sent last, or in any case with `force`, and the cover of the track when
/// Home Assistant does not have it yet. Does nothing while the current Link
/// session does not declare the media player.
pub async fn publish(app: &AppHandle, force: bool) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().await.clone();
    let hub = state.link_info();
    if cfg.transport != "link" || !declares_media_player(&cfg, hub.as_ref()) {
        return;
    }
    let with_art = takes_media_art(&cfg, hub.as_ref());
    let (reading, track) = match tokio::task::spawn_blocking(move || read(&cfg)).await {
        Ok(read) => read,
        Err(error) => {
            log::warn!("reading the media player failed: {error}");
            return;
        }
    };
    let changed = {
        let mut last = LAST_SENT.lock().unwrap_or_else(|e| e.into_inner());
        let changed = force || last.as_ref().is_none_or(|sent| reading.differs_from(sent));
        if changed {
            *last = Some(reading.clone());
        }
        changed
    };
    if changed && !crate::link::send(app, media_state_frame(&reading)).await {
        *LAST_SENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    if with_art {
        publish_art(app, track, force).await;
    }
}

/// Sends the cover of `track` when Home Assistant does not have it yet, or
/// takes the cover away when nothing has one. `new_session` forgets what the
/// session before was sent.
async fn publish_art(app: &AppHandle, track: Option<String>, new_session: bool) {
    // Reading and scaling the picture is WinRT work, off the runtime. The
    // lock is held across it so that two calls do not read the same cover.
    let step = tokio::task::spawn_blocking(move || {
        let mut tracker = ART.lock().unwrap_or_else(|e| e.into_inner());
        if new_session {
            tracker.reset();
        }
        tracker.next(track.as_deref(), crate::media_art::current)
    })
    .await;
    let frame = match step {
        Ok(ArtStep::Send(art)) => media_art_frame(Some(&art)),
        Ok(ArtStep::Remove) => media_art_frame(None),
        Ok(ArtStep::Keep) => return,
        Err(error) => {
            log::warn!("reading the media picture failed: {error}");
            return;
        }
    };
    if !crate::link::send(app, frame).await {
        ART.lock().unwrap_or_else(|e| e.into_inner()).reset();
    }
}

/// Runs a command for the media player. Refused while the current session
/// does not declare it, which also covers MQTT.
pub async fn handle_command(
    app: &AppHandle,
    action: &str,
    value: Option<&Value>,
) -> Result<(), String> {
    let (declared, volume_sensor) = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().await;
        (
            declares_media_player(&cfg, state.link_info().as_ref()),
            crate::sensors::is_enabled(&cfg, VOLUME_SENSOR),
        )
    };
    if !declared {
        return Err("the media player is not enabled on this computer".into());
    }
    let op = parse_media_command(action, value)?;
    tokio::task::spawn_blocking(move || run(op))
        .await
        .map_err(|e| e.to_string())??;
    // The volume control and the mute switch show the same values. The
    // volume goes out only while its sensor is on.
    let echoed = match op {
        MediaOp::SetVolume(_) if !volume_sensor => None,
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
        MediaOp::Seek(position_secs) => crate::media::seek(position_secs),
    }
}
