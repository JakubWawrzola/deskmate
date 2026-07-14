//! Media przez Windows SMTC (GlobalSystemMediaTransportControlsSession).
//! Odczyt aktualnego utworu + sterowanie play/pause/next/prev.

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub app: String,
    pub status: String,
}

#[cfg(windows)]
fn session() -> Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
    let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()?
        .get()
        .ok()?;
    mgr.GetCurrentSession().ok()
}

#[cfg(windows)]
pub fn current() -> Option<MediaInfo> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PS;
    let s = session()?;
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
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
        app: s.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default(),
        status: status.into(),
    })
}

#[cfg(not(windows))]
pub fn current() -> Option<MediaInfo> {
    None
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
        .get()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn control(_action: &str) -> Result<(), String> {
    Err("windows only".into())
}
