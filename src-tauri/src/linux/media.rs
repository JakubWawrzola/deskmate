//! The media player through MPRIS on the session bus: title, artist, player
//! and status, and play/pause, next and previous. Of several players the
//! one that plays wins, then one that is paused, then the first by name.

use dbus::arg::{PropMap, RefArg};
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::Connection;

use super::bus;
use crate::media::MediaInfo;

const PLAYER_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

/// One player on the bus.
struct Player {
    name: String,
    status: String,
}

fn players(connection: &Connection) -> Vec<Player> {
    bus::names_with_prefix(connection, PLAYER_PREFIX)
        .into_iter()
        .map(|name| {
            let status = connection
                .with_proxy(name.as_str(), PLAYER_PATH, bus::CALL_TIMEOUT)
                .get::<String>(PLAYER_INTERFACE, "PlaybackStatus")
                .unwrap_or_default();
            Player { name, status }
        })
        .collect()
}

/// The player that counts: playing, then paused, then the first.
fn pick(players: Vec<Player>) -> Option<Player> {
    let rank = |player: &Player| match player.status.as_str() {
        "Playing" => 0,
        "Paused" => 1,
        _ => 2,
    };
    players.into_iter().min_by_key(rank)
}

/// What plays now, or None without a player.
pub fn current() -> Option<MediaInfo> {
    let connection = bus::session().ok()?;
    let player = pick(players(&connection))?;
    let proxy = connection.with_proxy(player.name.as_str(), PLAYER_PATH, bus::CALL_TIMEOUT);
    let metadata: PropMap = proxy.get(PLAYER_INTERFACE, "Metadata").unwrap_or_default();
    let app = proxy
        .get::<String>(ROOT_INTERFACE, "Identity")
        .ok()
        .filter(|identity| !identity.trim().is_empty())
        .unwrap_or_else(|| short_name(&player.name));
    Some(MediaInfo {
        title: text(&metadata, "xesam:title"),
        artist: artists(&metadata),
        app,
        status: status_word(&player.status).into(),
    })
}

/// play_pause, play, pause, next or prev on the player that counts.
pub fn control(action: &str) -> Result<(), String> {
    let method = match action {
        "play_pause" => "PlayPause",
        "play" => "Play",
        "pause" => "Pause",
        "next" => "Next",
        "prev" => "Previous",
        _ => return Err(format!("unknown media action: {action}")),
    };
    let connection = bus::session()?;
    let player = pick(players(&connection)).ok_or("no media player is running")?;
    connection
        .with_proxy(player.name.as_str(), PLAYER_PATH, bus::CALL_TIMEOUT)
        .method_call::<(), _, _, _>(PLAYER_INTERFACE, method, ())
        .map_err(|e| format!("{}: {e}", short_name(&player.name)))
}

/// The status words of the Windows sensor.
fn status_word(mpris: &str) -> &'static str {
    match mpris {
        "Playing" => "playing",
        "Paused" => "paused",
        "Stopped" => "stopped",
        _ => "closed",
    }
}

/// "org.mpris.MediaPlayer2.firefox.instance_1_84" -> "firefox".
fn short_name(bus_name: &str) -> String {
    bus_name
        .strip_prefix(PLAYER_PREFIX)
        .unwrap_or(bus_name)
        .split('.')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn text(metadata: &PropMap, key: &str) -> String {
    metadata
        .get(key)
        .and_then(|value| value.0.as_str())
        .unwrap_or_default()
        .to_string()
}

/// `xesam:artist` is a list of names.
fn artists(metadata: &PropMap) -> String {
    let Some(value) = metadata.get("xesam:artist") else {
        return String::new();
    };
    if let Some(single) = value.0.as_str() {
        return single.to_string();
    }
    value
        .0
        .as_iter()
        .map(|names| {
            names
                .filter_map(|name| name.as_str().map(str::to_string))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::arg::Variant;

    fn player(name: &str, status: &str) -> Player {
        Player {
            name: name.into(),
            status: status.into(),
        }
    }

    #[test]
    fn the_playing_player_wins_over_paused_and_stopped_ones() {
        let picked = pick(vec![
            player("org.mpris.MediaPlayer2.a", "Stopped"),
            player("org.mpris.MediaPlayer2.b", "Paused"),
            player("org.mpris.MediaPlayer2.c", "Playing"),
        ]);
        assert_eq!(
            picked.map(|p| p.name).as_deref(),
            Some("org.mpris.MediaPlayer2.c")
        );
        let picked = pick(vec![
            player("org.mpris.MediaPlayer2.a", "Stopped"),
            player("org.mpris.MediaPlayer2.b", "Paused"),
        ]);
        assert_eq!(
            picked.map(|p| p.name).as_deref(),
            Some("org.mpris.MediaPlayer2.b")
        );
        assert!(pick(Vec::new()).is_none());
    }

    #[test]
    fn names_and_statuses_read_like_the_windows_sensors() {
        assert_eq!(
            short_name("org.mpris.MediaPlayer2.firefox.instance_1_84"),
            "firefox"
        );
        assert_eq!(short_name("org.mpris.MediaPlayer2.spotify"), "spotify");
        assert_eq!(status_word("Playing"), "playing");
        assert_eq!(status_word("Paused"), "paused");
        assert_eq!(status_word("Stopped"), "stopped");
        assert_eq!(status_word(""), "closed");
    }

    #[test]
    fn reads_title_and_artists_from_the_metadata() {
        let mut metadata = PropMap::new();
        metadata.insert(
            "xesam:title".into(),
            Variant(Box::new("Song".to_string()) as Box<dyn RefArg>),
        );
        metadata.insert(
            "xesam:artist".into(),
            Variant(Box::new(vec!["A".to_string(), "B".to_string()]) as Box<dyn RefArg>),
        );
        assert_eq!(text(&metadata, "xesam:title"), "Song");
        assert_eq!(artists(&metadata), "A, B");
        assert_eq!(text(&metadata, "xesam:album"), "");
        assert_eq!(artists(&PropMap::new()), "");
    }

    #[test]
    fn refuses_unknown_media_actions() {
        assert!(control("eject").is_err());
    }
}
