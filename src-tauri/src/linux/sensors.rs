//! The session sensors of the Linux preview, added to the machine sensors
//! by `sensors::Collector::collect`. Only what linux/entities.rs declares is
//! collected; a value that cannot be read is left out, so Home Assistant
//! shows the entity as unavailable instead of a made-up value.

use std::collections::HashMap;

use crate::config::AppConfig;
use crate::sensors::is_enabled;

/// The longest text sent for a media title or artist, as on Windows.
const MAX_TEXT_CHARS: usize = 250;

pub fn collect(cfg: &AppConfig, out: &mut HashMap<String, String>) {
    if is_enabled(cfg, "current_user") {
        if let Some(user) = user_name() {
            out.insert("current_user".into(), user);
        } else {
            out.remove("current_user");
        }
    }
    if is_enabled(cfg, "idle") || is_enabled(cfg, "last_active") {
        if let Some(idle) = super::session::idle_seconds() {
            if is_enabled(cfg, "idle") {
                out.insert("idle".into(), idle.to_string());
            }
            if is_enabled(cfg, "last_active") {
                out.insert("last_active".into(), crate::sensors::last_active(idle));
            }
        }
    }
    if is_enabled(cfg, "session_locked") {
        if let Some(locked) = super::session::locked() {
            out.insert("session_locked".into(), on_off(locked));
        }
    }
    // The mute switch follows changes made on the computer, as on Windows.
    if let Some(muted) = crate::sys_commands::audio_muted() {
        out.insert("audio_mute".into(), on_off(muted));
    }
    if is_enabled(cfg, "volume") {
        if let Some(volume) = crate::sys_commands::get_volume() {
            out.insert("volume".into(), volume.to_string());
        }
    }
    collect_media(cfg, out);
}

fn collect_media(cfg: &AppConfig, out: &mut HashMap<String, String>) {
    let keys = ["media_title", "media_artist", "media_app", "media_status"];
    if !keys.iter().any(|key| is_enabled(cfg, key)) {
        return;
    }
    let (title, artist, app, status) = match crate::media::current() {
        Some(media) => (media.title, media.artist, media.app, media.status),
        None => ("none".into(), "none".into(), "none".into(), "idle".into()),
    };
    for (key, value) in [
        ("media_title", shorten(&title)),
        ("media_artist", shorten(&artist)),
        ("media_app", app),
        ("media_status", status),
    ] {
        if is_enabled(cfg, key) {
            out.insert(key.into(), value);
        }
    }
}

/// The signed-in user's login name.
fn user_name() -> Option<String> {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("LOGNAME").ok())
        .filter(|name| !name.is_empty())
}

fn on_off(on: bool) -> String {
    if on { "ON" } else { "OFF" }.into()
}

fn shorten(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_media_texts_are_cut_like_on_windows() {
        assert_eq!(shorten("short"), "short");
        assert_eq!(shorten(&"é".repeat(300)).chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn switches_read_on_and_off() {
        assert_eq!(on_off(true), "ON");
        assert_eq!(on_off(false), "OFF");
    }
}
