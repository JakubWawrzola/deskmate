//! The `media_player` entity of a Link declaration: when it is declared, its
//! state object and the commands Home Assistant sends to it (docs/LINK.md,
//! "Media player"). Reading the media session and running the commands is up
//! to the client.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::frames::{feature, HubInfo};
use crate::config::AppConfig;

/// Key of the media player entity.
pub const MEDIA_KEY: &str = "media";
/// Home Assistant drops a text longer than this from the state object.
pub const MAX_MEDIA_TEXT_CHARS: usize = 256;
/// Longest track and furthest position Home Assistant accepts, in seconds.
pub const MAX_MEDIA_SECONDS: f64 = 604_800.0;
/// How far a reported position may be from where playback would have got to
/// by itself before it counts as a jump worth sending.
const POSITION_TOLERANCE_SECONDS: f64 = 2.0;
/// The capability an integration announces when it takes the cover of what
/// is playing in a `media_art` frame.
pub const CAP_MEDIA_ART: &str = "media_art";
/// Largest cover Home Assistant keeps, in bytes.
pub const MAX_MEDIA_ART_BYTES: usize = 256 * 1024;
/// Hex characters of the cover's name, out of its SHA-256.
const ART_HASH_CHARS: usize = 32;
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

/// Where a track is: its length, and a position with the moment it was read.
/// Home Assistant works out the current position from the two, so a new one
/// is worth sending only when playback jumped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    pub duration_secs: f64,
    pub position_secs: f64,
    /// Unix time, in seconds, at which `position_secs` was true.
    pub position_at: f64,
}

impl Timeline {
    /// A timeline Home Assistant would accept, or none. A player that reports
    /// no length (a live stream, or an app that does not fill the timeline
    /// in) has no timeline at all.
    pub fn checked(duration_secs: f64, position_secs: f64, position_at: f64) -> Option<Self> {
        let valid = duration_secs.is_finite()
            && position_secs.is_finite()
            && position_at.is_finite()
            && duration_secs > 0.0
            && duration_secs <= MAX_MEDIA_SECONDS
            && position_at > 0.0;
        valid.then(|| Self {
            duration_secs,
            // A player may report a position a moment past the end.
            position_secs: position_secs.clamp(0.0, duration_secs),
            position_at,
        })
    }

    /// Whether `self` says the same as `earlier` once the time between the
    /// two readings is taken into account. While playing, the position is
    /// expected to have moved on by that time; otherwise to have stayed.
    pub fn continues(&self, earlier: &Timeline, playing: bool) -> bool {
        let elapsed = if playing {
            self.position_at - earlier.position_at
        } else {
            0.0
        };
        self.duration_secs == earlier.duration_secs
            && (self.position_secs - (earlier.position_secs + elapsed)).abs()
                <= POSITION_TOLERANCE_SECONDS
    }
}

/// What the client read from the current media session.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaSession {
    /// As the media status sensor reports it, such as "playing".
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The app that owns the session.
    pub app: String,
    pub timeline: Option<Timeline>,
}

/// The state object of the media player.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaPlayerState {
    pub state: PlaybackState,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub app: Option<String>,
    /// 0 to 1.
    pub volume: Option<f64>,
    pub muted: Option<bool>,
    pub timeline: Option<Timeline>,
}

impl MediaPlayerState {
    /// The state for what the client read. A text whose media sensor the user
    /// left off stays out, and so does an empty one; the album follows the
    /// title's switch, as both say what is playing. The volume stays out
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
            album: text("media_title", |session| &session.album),
            app: text("media_app", |session| &session.app),
            volume: volume_percent
                .filter(|_| crate::sensors::is_enabled(cfg, VOLUME_SENSOR))
                .map(|percent| f64::from(percent.min(100)) / 100.0),
            muted,
            timeline: session.and_then(|session| session.timeline),
        }
    }

    pub fn to_value(&self) -> Value {
        let timeline = self.timeline.as_ref();
        json!({
            "state": self.state.as_str(),
            "title": self.title,
            "artist": self.artist,
            "album": self.album,
            "app": self.app,
            "volume": self.volume,
            "muted": self.muted,
            "duration": timeline.map(|timeline| tenths(timeline.duration_secs)),
            "position": timeline.map(|timeline| tenths(timeline.position_secs)),
            "position_at": timeline.map(|timeline| tenths(timeline.position_at)),
        })
    }

    /// Whether `self` is worth sending after `sent`. Everything counts but a
    /// position that only moved on with the clock: the reading differs on
    /// every tick then, and Home Assistant already knows where the track is.
    pub fn differs_from(&self, sent: &MediaPlayerState) -> bool {
        let same_timeline = match (&self.timeline, &sent.timeline) {
            (None, None) => true,
            (Some(now), Some(earlier)) => {
                now.continues(earlier, self.state == PlaybackState::Playing)
            }
            _ => false,
        };
        let rest = |state: &MediaPlayerState| MediaPlayerState {
            timeline: None,
            ..state.clone()
        };
        !same_timeline || rest(self) != rest(sent)
    }
}

/// A number of seconds to one decimal place: more would only make every
/// frame look different.
fn tenths(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
}

/// A `state` frame that carries only the media player.
pub fn media_state_frame(state: &MediaPlayerState) -> Value {
    json!({"t": "state", "s": {MEDIA_KEY: state.to_value()}})
}

/// The cover of what is playing, as a JPEG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaArt {
    pub jpeg: Vec<u8>,
    /// The name Home Assistant knows the picture by; see `art_hash`.
    pub hash: String,
}

/// The name of a picture: the start of its SHA-256 in hex. Home Assistant
/// fetches a picture again only when the name changes.
pub fn art_hash(picture: &[u8]) -> String {
    let mut hash: String = Sha256::digest(picture)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    hash.truncate(ART_HASH_CHARS);
    hash
}

/// The `media_art` frame: a picture, or with `None` the news that nothing
/// has one.
pub fn media_art_frame(art: Option<&MediaArt>) -> Value {
    match art {
        Some(art) => json!({
            "t": "media_art",
            "hash": art.hash,
            "mime": "image/jpeg",
            "data": B64.encode(&art.jpeg),
        }),
        None => json!({"t": "media_art", "hash": null, "data": null}),
    }
}

/// Whether the cover may go to Home Assistant at all: only to an integration
/// that asked for it, and only next to a declared media player.
pub fn takes_media_art(cfg: &AppConfig, hub: Option<&HubInfo>) -> bool {
    declares_media_player(cfg, hub) && hub.is_some_and(|hub| hub.has_cap(CAP_MEDIA_ART))
}

/// What identifies the track a cover belongs to, or `None` when no cover is
/// to be shown: nothing plays, or the user keeps the title to themselves, and
/// a cover says as much as a title does.
pub fn art_track(cfg: &AppConfig, session: Option<&MediaSession>) -> Option<String> {
    let session = session?;
    if !crate::sensors::is_enabled(cfg, "media_title")
        || PlaybackState::from_status(Some(&session.status)) == PlaybackState::Off
    {
        return None;
    }
    Some(format!(
        "{}\n{}\n{}\n{}",
        session.app, session.artist, session.album, session.title
    ))
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
    /// Seconds into the track.
    Seek(f64),
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
        "seek" => value
            .get("position")
            .and_then(Value::as_f64)
            .filter(|position| (0.0..=MAX_MEDIA_SECONDS).contains(position))
            .map(MediaOp::Seek)
            .ok_or_else(|| "media: position must be a number of seconds from 0".into()),
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
            album: "Record".into(),
            app: "Player.exe".into(),
            timeline: Timeline::checked(241.52, 63.21, 1_795_348_800.0),
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
                "album": "Record",
                "app": "Player.exe",
                "volume": 0.4,
                "muted": false,
                "duration": 241.5,
                "position": 63.2,
                "position_at": 1_795_348_800.0,
            }}})
        );
    }

    #[test]
    fn a_session_without_a_timeline_sends_nulls_for_it() {
        let cfg = with_sensors(&MEDIA_SENSORS);
        let live = MediaSession {
            timeline: None,
            ..session()
        };
        let value = MediaPlayerState::new(&cfg, Some(&live), None, None).to_value();
        for field in ["duration", "position", "position_at"] {
            assert_eq!(value[field], Value::Null, "{field}");
        }
        assert_eq!(value["album"], "Record");
    }

    #[test]
    fn a_cover_is_named_by_the_start_of_its_hash_and_sent_as_base64() {
        // SHA-256 of "abc".
        assert_eq!(art_hash(b"abc"), "ba7816bf8f01cfea414140de5dae2223");
        let art = MediaArt {
            jpeg: vec![0xff, 0xd8, 0xff, 0xe0],
            hash: art_hash(&[0xff, 0xd8, 0xff, 0xe0]),
        };
        assert_eq!(
            media_art_frame(Some(&art)),
            json!({"t": "media_art", "hash": art.hash, "mime": "image/jpeg", "data": "/9j/4A=="})
        );
        assert_eq!(
            media_art_frame(None),
            json!({"t": "media_art", "hash": null, "data": null})
        );
    }

    #[test]
    fn the_cover_goes_only_to_an_integration_that_takes_it() {
        let cfg = with_sensors(&["media_status", "media_title"]);
        assert!(takes_media_art(
            &cfg,
            Some(&hub(&["info", "media_player", "media_art"]))
        ));
        assert!(!takes_media_art(
            &cfg,
            Some(&hub(&["info", "media_player"]))
        ));
        assert!(!takes_media_art(&cfg, Some(&hub(&["info", "media_art"]))));
        assert!(!takes_media_art(&cfg, None));
        // Without the media player there is nothing to show it on.
        assert!(!takes_media_art(
            &AppConfig::default(),
            Some(&hub(&["info", "media_player", "media_art"]))
        ));
    }

    #[test]
    fn a_cover_belongs_to_a_track_only_while_the_title_may_be_shown() {
        let shown = with_sensors(&["media_status", "media_title"]);
        let track = art_track(&shown, Some(&session()));
        assert_eq!(track.as_deref(), Some("Player.exe\nBand\nRecord\nSong"));
        let other = MediaSession {
            title: "Other".into(),
            ..session()
        };
        assert_ne!(art_track(&shown, Some(&other)), track);

        // The title sensor is off: a cover would give the title away.
        let hidden = with_sensors(&["media_status", "media_artist"]);
        assert_eq!(art_track(&hidden, Some(&session())), None);
        // Nothing plays.
        assert_eq!(art_track(&shown, None), None);
        let closed = MediaSession {
            status: "closed".into(),
            ..session()
        };
        assert_eq!(art_track(&shown, Some(&closed)), None);
    }

    #[test]
    fn only_a_usable_timeline_is_kept() {
        let at = 1_795_348_800.0;
        assert!(Timeline::checked(240.0, 10.0, at).is_some());
        // No length: a live stream, or a player that does not say.
        assert_eq!(Timeline::checked(0.0, 0.0, at), None);
        assert_eq!(Timeline::checked(-1.0, 0.0, at), None);
        assert_eq!(Timeline::checked(MAX_MEDIA_SECONDS + 1.0, 0.0, at), None);
        assert_eq!(Timeline::checked(f64::NAN, 0.0, at), None);
        assert_eq!(Timeline::checked(240.0, f64::INFINITY, at), None);
        // The player never set the time of its reading.
        assert_eq!(Timeline::checked(240.0, 10.0, 0.0), None);
        // A position outside the track is brought back into it.
        let past = Timeline::checked(240.0, 240.4, at).unwrap();
        assert_eq!(past.position_secs, 240.0);
        let before = Timeline::checked(240.0, -0.2, at).unwrap();
        assert_eq!(before.position_secs, 0.0);
    }

    #[test]
    fn the_album_follows_the_title_switch() {
        let cfg = with_sensors(&["media_status", "media_artist"]);
        let state = MediaPlayerState::new(&cfg, Some(&session()), None, None);
        assert_eq!(state.album, None);
        let cfg = with_sensors(&["media_status", "media_title"]);
        let state = MediaPlayerState::new(&cfg, Some(&session()), None, None);
        assert_eq!(state.album.as_deref(), Some("Record"));
    }

    fn at(position_secs: f64, position_at: f64, status: &str) -> MediaPlayerState {
        let cfg = with_sensors(&MEDIA_SENSORS);
        let session = MediaSession {
            status: status.into(),
            timeline: Timeline::checked(240.0, position_secs, position_at),
            ..session()
        };
        MediaPlayerState::new(&cfg, Some(&session), Some(40), Some(false))
    }

    #[test]
    fn a_position_that_moved_on_with_the_clock_is_not_sent_again() {
        let start = 1_795_348_800.0;
        let sent = at(10.0, start, "playing");
        // Ten seconds later the player reports ten seconds more.
        assert!(!at(20.0, start + 10.0, "playing").differs_from(&sent));
        assert!(!at(21.5, start + 10.0, "playing").differs_from(&sent));
        // The same reading again.
        assert!(!at(10.0, start, "playing").differs_from(&sent));
    }

    #[test]
    fn a_jump_a_pause_or_another_track_is_sent() {
        let start = 1_795_348_800.0;
        let sent = at(10.0, start, "playing");
        // A seek: the position is not where playback would have got to.
        assert!(at(120.0, start + 10.0, "playing").differs_from(&sent));
        assert!(at(10.0, start + 10.0, "playing").differs_from(&sent));
        // Paused: the state itself changed.
        assert!(at(20.0, start + 10.0, "paused").differs_from(&sent));
        // While paused the position stays; a move is a seek.
        let paused = at(20.0, start, "paused");
        assert!(!at(20.0, start + 30.0, "paused").differs_from(&paused));
        assert!(at(50.0, start + 30.0, "paused").differs_from(&paused));
        // The timeline went away or came back.
        let mut without = sent.clone();
        without.timeline = None;
        assert!(without.differs_from(&sent));
        assert!(sent.differs_from(&without));
        // Any other field.
        let mut louder = sent.clone();
        louder.volume = Some(0.9);
        assert!(louder.differs_from(&sent));
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
        assert_eq!(
            parse(json!({"op": "seek", "position": 120})),
            Ok(MediaOp::Seek(120.0))
        );
        assert_eq!(
            parse(json!({"op": "seek", "position": 0.5})),
            Ok(MediaOp::Seek(0.5))
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
            json!({"op": "seek"}),
            json!({"op": "seek", "position": "12"}),
            json!({"op": "seek", "position": -1}),
            json!({"op": "seek", "position": 604_801}),
        ];
        for value in refused {
            assert!(parse_media_command("set", Some(&value)).is_err(), "{value}");
        }
        assert!(parse_media_command("set", None).is_err());
        assert!(parse_media_command("press", Some(&json!({"op": "play"}))).is_err());
    }
}
