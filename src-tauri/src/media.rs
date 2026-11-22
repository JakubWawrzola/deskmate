//! Media przez Windows SMTC (GlobalSystemMediaTransportControlsSession).
//! Odczyt aktualnego utworu + sterowanie play/pause/next/prev.

use deskmate_core::link::media::Timeline;

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub status: String,
    /// The length of the track and where it is, when the player says.
    pub timeline: Option<Timeline>,
}

/// Windows counts time in units of 100 nanoseconds.
#[cfg(windows)]
const TICKS_PER_SECOND: f64 = 10_000_000.0;
/// Seconds between 1601-01-01, where Windows starts counting, and
/// 1970-01-01, where Unix time does.
#[cfg(windows)]
const WINDOWS_TO_UNIX_EPOCH_SECS: f64 = 11_644_473_600.0;

#[cfg(windows)]
fn session() -> Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
    let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()?
        .join()
        .ok()?;
    mgr.GetCurrentSession().ok()
}

#[cfg(windows)]
pub fn current() -> Option<MediaInfo> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PS;
    let s = session()?;
    let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let status = match s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? {
        PS::Playing => "playing",
        PS::Paused => "paused",
        PS::Stopped => "stopped",
        PS::Changing => "changing",
        PS::Opened => "opened",
        _ => "closed",
    };
    Some(MediaInfo {
        title: props.Title().map(|s| s.to_string()).unwrap_or_default(),
        artist: props.Artist().map(|s| s.to_string()).unwrap_or_default(),
        album: props
            .AlbumTitle()
            .map(|s| s.to_string())
            .unwrap_or_default(),
        app: s
            .SourceAppUserModelId()
            .map(|s| s.to_string())
            .unwrap_or_default(),
        status: status.into(),
        timeline: timeline(&s),
    })
}

/// The session's timeline. A player that does not fill it in reports zeros,
/// which `Timeline::checked` turns into none.
#[cfg(windows)]
fn timeline(
    session: &windows::Media::Control::GlobalSystemMediaTransportControlsSession,
) -> Option<Timeline> {
    let properties = session.GetTimelineProperties().ok()?;
    let start = properties.StartTime().ok()?.Duration;
    let end = properties.EndTime().ok()?.Duration;
    let position = properties.Position().ok()?.Duration;
    let updated = properties.LastUpdatedTime().ok()?.UniversalTime;
    Timeline::checked(
        (end - start) as f64 / TICKS_PER_SECOND,
        (position - start) as f64 / TICKS_PER_SECOND,
        updated as f64 / TICKS_PER_SECOND - WINDOWS_TO_UNIX_EPOCH_SECS,
    )
}

/// Moves playback to `position_secs` into the track.
#[cfg(windows)]
pub fn seek(position_secs: f64) -> Result<(), String> {
    let s = session().ok_or("no active media session")?;
    let start = s
        .GetTimelineProperties()
        .and_then(|properties| properties.StartTime())
        .map(|start| start.Duration)
        .unwrap_or(0);
    let target = start + (position_secs * TICKS_PER_SECOND) as i64;
    let accepted = s
        .TryChangePlaybackPositionAsync(target)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|e| e.to_string())?;
    if accepted {
        Ok(())
    } else {
        Err("the player did not accept the position".into())
    }
}

/// MPRIS on Linux (linux/media.rs).
#[cfg(not(windows))]
pub fn seek(position_secs: f64) -> Result<(), String> {
    crate::linux::media::seek(position_secs)
}

/// MPRIS on Linux (linux/media.rs).
#[cfg(not(windows))]
pub fn current() -> Option<MediaInfo> {
    crate::linux::media::current()
}

#[cfg(windows)]
pub fn control(action: &str) -> Result<(), String> {
    let s = session().ok_or("no active media session")?;
    let op = match action {
        "play_pause" => s.TryTogglePlayPauseAsync(),
        "play" => s.TryPlayAsync(),
        "pause" => s.TryPauseAsync(),
        "next" => s.TrySkipNextAsync(),
        "prev" => s.TrySkipPreviousAsync(),
        _ => return Err(format!("unknown media action: {action}")),
    };
    op.map_err(|e| e.to_string())?
        .join()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn control(action: &str) -> Result<(), String> {
    crate::linux::media::control(action)
}
