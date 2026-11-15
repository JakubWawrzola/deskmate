//! The `media_player` entity of a Link declaration: when it is declared, its
//! state object and the commands Home Assistant sends to it (docs/LINK.md,
//! "Media player"). Reading the media session and running the commands is up
//! to the client.

use serde_json::{json, Value};

use super::frames::{feature, HubInfo};
use crate::config::AppConfig;

/// Key of the media player entity.
pub const MEDIA_KEY: &str = "media";
/// Home Assistant drops a text longer than this from the state object.
pub const MAX_MEDIA_TEXT_CHARS: usize = 256;
/// The media sensors. The media player shows the part of each one only
/// while its switch is on.
pub const MEDIA_SENSORS: [&str; 4] = ["media_title", "media_artist", "media_app", "media_status"];
/// The sensor whose switch decides whether the media player exists: its
/// `state` is what this sensor reports.
pub const MEDIA_STATUS_SENSOR: &str = "media_status";
/// The sensor whose switch decides whether the media player reports the
/// volume.
pub const VOLUME_SENSOR: &str = "volume";

/// The descriptor in a `declare` frame.
pub fn media_entity() -> Value {
    json!({"key": MEDIA_KEY, "kind": "media_player", "name": "Media"})
}

/// Whether a declaration includes the media player. Home Assistant must have
/// announced the entity kind, and the user must have turned on the media
/// status sensor: the entity's state tells whether something is playing,
/// which that sensor is off by default for.
pub fn declares_media_player(cfg: &AppConfig, hub: Option<&HubInfo>) -> bool {
    hub.is_some_and(|hub| hub.has_cap(feature::MEDIA_PLAYER))
        && crate::sensors::is_enabled(cfg, MEDIA_STATUS_SENSOR)
}

/// The `state` field of the media player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Idle,
    Off,
}

impl PlaybackState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Idle => "idle",
            Self::Off => "off",
        }
    }

    /// From the status that the media status sensor reports. A session that
    /// is stopped, opening or changing tracks is idle; a closed one, or none
    /// at all, is off.
    pub fn from_status(status: Option<&str>) -> Self {
        match status {
            Some("playing") => Self::Playing,
            Some("paused") => Self::Paused,
            Some("stopped" | "changing" | "opened") => Self::Idle,
            _ => Self::Off,
        }
    }
}

/// What the client read from the current media session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSession {
    /// As the media status sensor reports it, such as "playing".
    pub status: String,
    pub title: String,
    pub artist: String,
    /// The app that owns the session.
    pub app: String,
}

/// The state object of the media player.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaPlayerState {
    pub state: PlaybackState,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub app: Option<String>,
    /// 0 to 1.
    pub volume: Option<f64>,
    pub muted: Option<bool>,
}

impl MediaPlayerState {
    /// The state for what the client read. A text whose media sensor the user
    /// left off stays out, and so does an empty one. The volume stays out
    /// while the volume sensor is off; the mute state is always there, like
    /// the mute switch's.
    pub fn new(
        cfg: &AppConfig,
        session: Option<&MediaSession>,
        volume_percent: Option<u8>,
        muted: Option<bool>,
    ) -> Self {
        let text = |sensor: &str, value: fn(&MediaSession) -> &str| {
            session
                .filter(|_| crate::sensors::is_enabled(cfg, sensor))
                .map(value)
                .filter(|value| !value.is_empty())
                .map(|value| value.chars().take(MAX_MEDIA_TEXT_CHARS).collect())
        };
        Self {
            state: PlaybackState::from_status(session.map(|session| session.status.as_str())),
            title: text("media_title", |session| &session.title),
            artist: text("media_artist", |session| &session.artist),
            app: text("media_app", |session| &session.app),
            volume: volume_percent
                .filter(|_| crate::sensors::is_enabled(cfg, VOLUME_SENSOR))
                .map(|percent| f64::from(percent.min(100)) / 100.0),
            muted,
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "state": self.state.as_str(),
            "title": self.title,
            "artist": self.artist,
            "app": self.app,
            "volume": self.volume,
            "muted": self.muted,
        })
    }
}

/// A `state` frame that carries only the media player.
pub fn media_state_frame(state: &MediaPlayerState) -> Value {
    json!({"t": "state", "s": {MEDIA_KEY: state.to_value()}})
}

/// What Home Assistant asks the media player to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaOp {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// 0 to 1.
    SetVolume(f64),
    Mute(bool),
}

/// Reads the `action` and `value` of a `cmd` for the media player. Anything
/// but `set` with a known `op` and, where the op needs one, a valid argument
/// is refused. The error goes back to Home Assistant and never repeats what
/// was received.
pub fn parse_media_command(action: &str, value: Option<&Value>) -> Result<MediaOp, String> {
    if action != "set" {
        return Err("media: the media player takes set only".into());
    }
    let value = value
        .and_then(Value::as_object)
        .ok_or("media: the value must be an object")?;
    let op = value
        .get("op")
        .and_then(Value::as_str)
        .ok_or("media: op is missing")?;
    match op {
        "play_pause" => Ok(MediaOp::PlayPause),
        "play" => Ok(MediaOp::Play),
        "pause" => Ok(MediaOp::Pause),
        "next" => Ok(MediaOp::Next),
        "previous" => Ok(MediaOp::Previous),
        "set_volume" => value
            .get("volume")
            .and_then(Value::as_f64)
            .filter(|volume| (0.0..=1.0).contains(volume))
            .map(MediaOp::SetVolume)
            .ok_or_else(|| "media: volume must be a number from 0 to 1".into()),
        "mute" => value
            .get("muted")
            .and_then(Value::as_bool)
            .map(MediaOp::Mute)
            .ok_or_else(|| "media: muted must be true or false".into()),
        _ => Err("media: unknown op".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub(caps: &[&str]) -> HubInfo {
        HubInfo {
            integration: "0.9.0".into(),
            caps: caps.iter().map(|cap| cap.to_string()).collect(),
        }
    }

    fn with_sensors(ids: &[&str]) -> AppConfig {
        let mut cfg = AppConfig::default();
        for id in ids {
            cfg.sensors_enabled.insert(id.to_string(), true);
        }
        cfg
    }

    fn session() -> MediaSession {
        MediaSession {
            status: "playing".into(),
            title: "Song".into(),
            artist: "Band".into(),
            app: "Player.exe".into(),
        }
    }

    #[test]
    fn declared_only_with_the_cap_and_the_media_status_sensor_on() {
        let on = with_sensors(&["media_status"]);
        let off = AppConfig::default();
        let new_hub = hub(&["info", "media_player"]);
        let old_hub = hub(&["info", "scoped_declare", "bye"]);

        assert!(declares_media_player(&on, Some(&new_hub)));
        assert!(!declares_media_player(&on, Some(&old_hub)));
        assert!(!declares_media_player(&on, None));
        assert!(!declares_media_player(&off, Some(&new_hub)));
    }

    /// The player's state is what the media status sensor reports, so the
    /// other media sensors alone do not make it.
    #[test]
    fn the_other_media_sensors_do_not_declare_it_without_media_status() {
        let new_hub = hub(&["info", "media_player"]);
        let others = with_sensors(&["media_title", "media_artist", "media_app"]);
        assert!(!declares_media_player(&others, Some(&new_hub)));
        for sensor in ["media_title", "media_artist", "media_app"] {
            assert!(!declares_media_player(
                &with_sensors(&[sensor]),
                Some(&new_hub)
            ));
        }
    }

    #[test]
    fn the_volume_follows_the_volume_sensor() {
        let mut cfg = with_sensors(&["media_status"]);
        let on = MediaPlayerState::new(&cfg, Some(&session()), Some(40), Some(true));
        assert_eq!(on.volume, Some(0.4));

        cfg.sensors_enabled.insert(VOLUME_SENSOR.into(), false);
        let off = MediaPlayerState::new(&cfg, Some(&session()), Some(40), Some(true));
        assert_eq!(off.to_value()["volume"], Value::Null);
        // The mute switch is published whatever the sensors say.
        assert_eq!(off.muted, Some(true));
    }

    #[test]
    fn the_entity_is_a_media_player_named_media() {
        assert_eq!(
            media_entity(),
            json!({"key": "media", "kind": "media_player", "name": "Media"})
        );
    }

    #[test]
    fn the_state_object_has_every_field_of_the_contract() {
        let cfg = with_sensors(&MEDIA_SENSORS);
        let state = MediaPlayerState::new(&cfg, Some(&session()), Some(40), Some(false));
        assert_eq!(
            media_state_frame(&state),
            json!({"t": "state", "s": {"media": {
                "state": "playing",
                "title": "Song",
                "artist": "Band",
                "app": "Player.exe",
                "volume": 0.4,
                "muted": false,
            }}})
        );
    }

    #[test]
    fn texts_follow_their_sensor_switches() {
        let cfg = with_sensors(&["media_status"]);
        let state = MediaPlayerState::new(&cfg, Some(&session()), None, None);
        let value = state.to_value();
        assert_eq!(value["state"], "playing");
        for field in ["title", "artist", "app", "volume", "muted"] {
            assert_eq!(value[field], Value::Null, "{field}");
        }

        let cfg = with_sensors(&["media_artist"]);
        let value = MediaPlayerState::new(&cfg, Some(&session()), None, None).to_value();
        assert_eq!(value["title"], Value::Null);
        assert_eq!(value["artist"], "Band");
    }

    #[test]
    fn long_and_empty_texts_are_cut_or_left_out() {
        let cfg = with_sensors(&MEDIA_SENSORS);
        let long = MediaSession {
            title: "\u{e9}".repeat(300),
            artist: String::new(),
            ..session()
        };
        let state = MediaPlayerState::new(&cfg, Some(&long), None, None);
        let title = state.title.unwrap_or_default();
        assert_eq!(title.chars().count(), MAX_MEDIA_TEXT_CHARS);
        assert_eq!(state.artist, None);
    }

    #[test]
    fn playback_status_maps_onto_the_four_states() {
        let cases = [
            (Some("playing"), "playing"),
            (Some("paused"), "paused"),
            (Some("stopped"), "idle"),
            (Some("changing"), "idle"),
            (Some("opened"), "idle"),
            (Some("closed"), "off"),
            (Some("whatever"), "off"),
            (None, "off"),
        ];
        for (status, expected) in cases {
            assert_eq!(PlaybackState::from_status(status).as_str(), expected);
        }
        let cfg = with_sensors(&MEDIA_SENSORS);
        let nothing = MediaPlayerState::new(&cfg, None, Some(100), Some(true));
        assert_eq!(nothing.state, PlaybackState::Off);
        assert_eq!(nothing.title, None);
        assert_eq!(nothing.volume, Some(1.0));
    }

    #[test]
    fn reads_every_op() {
        let parse = |value: Value| parse_media_command("set", Some(&value));
        assert_eq!(parse(json!({"op": "play_pause"})), Ok(MediaOp::PlayPause));
        assert_eq!(parse(json!({"op": "play"})), Ok(MediaOp::Play));
        assert_eq!(parse(json!({"op": "pause"})), Ok(MediaOp::Pause));
        assert_eq!(parse(json!({"op": "next"})), Ok(MediaOp::Next));
        assert_eq!(parse(json!({"op": "previous"})), Ok(MediaOp::Previous));
        assert_eq!(
            parse(json!({"op": "set_volume", "volume": 0.25})),
            Ok(MediaOp::SetVolume(0.25))
        );
        assert_eq!(
            parse(json!({"op": "set_volume", "volume": 1})),
            Ok(MediaOp::SetVolume(1.0))
        );
        assert_eq!(
            parse(json!({"op": "set_volume", "volume": 0})),
            Ok(MediaOp::SetVolume(0.0))
        );
        assert_eq!(
            parse(json!({"op": "mute", "muted": true})),
            Ok(MediaOp::Mute(true))
        );
    }

    #[test]
    fn refuses_what_is_not_an_op() {
        let refused = [
            json!(null),
            json!("play"),
            json!(["play"]),
            json!({}),
            json!({"op": 1}),
            json!({"op": "Play"}),
            json!({"op": "play "}),
            json!({"op": "stop"}),
            json!({"op": "prev"}),
            json!({"op": "set_volume"}),
            json!({"op": "set_volume", "volume": "0.5"}),
            json!({"op": "set_volume", "volume": -0.01}),
            json!({"op": "set_volume", "volume": 1.01}),
            json!({"op": "set_volume", "volume": 50}),
            json!({"op": "mute"}),
            json!({"op": "mute", "muted": "true"}),
            json!({"op": "mute", "muted": 1}),
        ];
        for value in refused {
            assert!(parse_media_command("set", Some(&value)).is_err(), "{value}");
        }
        assert!(parse_media_command("set", None).is_err());
        assert!(parse_media_command("press", Some(&json!({"op": "play"}))).is_err());
    }
}
