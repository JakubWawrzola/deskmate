//! Session frames that every client builds and reads the same way: `state`
//! and `info` (docs/LINK.md, "Session frames"). The `declare` frame is in
//! `discovery.rs` and `machine.rs`.

use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// The `state` frame for collected values. Home Assistant expects a typed
/// value for some entities, where the collectors and MQTT use text.
pub fn state_frame(values: &HashMap<String, String>) -> Value {
    json!({"t": "state", "s": typed_states(values)})
}

fn typed_states(values: &HashMap<String, String>) -> Value {
    let mut states = Map::new();
    for (key, value) in values {
        let component = crate::sensors::SENSOR_DEFS
            .iter()
            .find(|definition| definition.id == key)
            .map(|definition| definition.component);
        let typed = match component {
            Some("binary_sensor") => Value::Bool(
                value.eq_ignore_ascii_case("ON")
                    || value.eq_ignore_ascii_case("true")
                    || value == "1",
            ),
            Some("number") => value
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(value.clone())),
            _ if matches!(
                key.as_str(),
                "keep_awake"
                    | "audio_mute"
                    | "mic_mute"
                    | "family_rules"
                    | "block_apps"
                    | "local_extra_time"
            ) =>
            {
                Value::Bool(value.eq_ignore_ascii_case("ON"))
            }
            // no daily limit: unknown in Home Assistant
            _ if key == "screen_time_left" && value == "None" => Value::Null,
            // The volume and the mute state of a single program; unknown
            // while the program plays no sound.
            _ if key.starts_with(crate::app_volume::VOLUME_PREFIX)
                || key.starts_with(crate::app_volume::MUTE_PREFIX) =>
            {
                if value == crate::app_volume::NO_SESSION {
                    Value::Null
                } else if key.starts_with(crate::app_volume::MUTE_PREFIX) {
                    Value::Bool(value.eq_ignore_ascii_case("ON"))
                } else {
                    value
                        .parse::<f64>()
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map_or(Value::Null, Value::Number)
                }
            }
            _ if matches!(key.as_str(), "screen_time_limit" | "screen_time_bonus") => value
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(value.clone())),
            // on/off custom sensors; Home Assistant reads the text "OFF" as true
            _ if key.starts_with("csb_") => Value::Bool(value.eq_ignore_ascii_case("ON")),
            _ => Value::String(value.clone()),
        };
        states.insert(key.clone(), typed);
    }
    Value::Object(states)
}

/// What kind of program a client is, for its `info` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientRole {
    /// The desktop app, running in the user's session.
    App,
    /// The Windows service.
    Service,
    /// The Linux daemon.
    Daemon,
}

impl ClientRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Service => "service",
            Self::Daemon => "daemon",
        }
    }
}

/// The system a client runs on, for its `info` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientPlatform {
    Windows,
    Linux,
}

impl ClientPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

/// Optional protocol features a client can name in the `features` field of
/// its `info` frame. Each name is also the `caps` item with which Home
/// Assistant announces that it understands the same feature.
pub mod feature {
    /// The `media_player` entity kind and its commands.
    pub const MEDIA_PLAYER: &str = "media_player";
    /// `tag` and `priority` on `notify`, and the `notify_clear` frame.
    pub const NOTIFY_TAG: &str = "notify_tag";
}

/// The `info` frame: what this client is. A client sends it right after
/// `welcome`, before `declare`. An integration older than 0.9.0 does not know
/// the frame, ignores it and never answers. `features` lists the optional
/// parts of the protocol that this client implements; an empty list leaves
/// the field out, so the frame stays what older clients sent.
pub fn info_frame(role: ClientRole, platform: ClientPlatform, features: &[&str]) -> Value {
    let mut frame = json!({
        "t": "info",
        "role": role.as_str(),
        "platform": platform.as_str(),
        "app": env!("CARGO_PKG_VERSION"),
    });
    if !features.is_empty() {
        frame["features"] = json!(features);
    }
    frame
}

const MAX_VERSION_CHARS: usize = 40;
const MAX_CAPS: usize = 32;
const MAX_CAP_CHARS: usize = 32;

/// What Home Assistant says about itself when it answers `info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubInfo {
    /// Version of the integration, such as "0.9.0".
    pub integration: String,
    /// The optional frame types the integration understands.
    pub caps: Vec<String>,
}

impl HubInfo {
    pub fn has_cap(&self, cap: &str) -> bool {
        self.caps.iter().any(|known| known == cap)
    }
}

/// Reads Home Assistant's answer to `info`. None when `integration` or `caps`
/// is missing or not in the expected form: the frame is then ignored as a
/// whole. Other fields are ignored. The caller must not log the frame; what
/// this returns has passed the checks and is safe to show.
pub fn parse_info(frame: &Value) -> Option<HubInfo> {
    let integration = frame.get("integration")?.as_str()?;
    if !is_version(integration) {
        return None;
    }
    let caps = frame.get("caps")?.as_array()?;
    if caps.len() > MAX_CAPS {
        return None;
    }
    let caps = caps
        .iter()
        .map(|cap| cap.as_str().filter(|cap| is_cap(cap)).map(str::to_string))
        .collect::<Option<Vec<String>>>()?;
    Some(HubInfo {
        integration: integration.to_string(),
        caps,
    })
}

/// Three numbers separated by dots, optionally followed by `-` or `+` and
/// letters, digits, dots or hyphens. At most 40 characters.
fn is_version(text: &str) -> bool {
    if text.len() > MAX_VERSION_CHARS {
        return false;
    }
    let (numbers, suffix) = match text.find(['-', '+']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let mut parts = numbers.split('.');
    let three_numbers = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    });
    three_numbers
        && parts.next().is_none()
        && suffix.is_none_or(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
}

/// A short lowercase identifier: a letter, then letters, digits or
/// underscores. At most 32 characters.
fn is_cap(text: &str) -> bool {
    text.len() <= MAX_CAP_CHARS
        && text.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn state_frame_types_the_values_home_assistant_reads_as_typed() {
        let frame = state_frame(&values(&[
            ("cpu", "12.5"),
            ("ac_power", "ON"),
            ("session_locked", "OFF"),
            ("volume", "40"),
            ("keep_awake", "ON"),
            ("csb_game", "OFF"),
            ("cs_build", "OFF"),
            ("screen_time_left", "None"),
            ("screen_time_limit", "90"),
        ]));
        assert_eq!(
            frame,
            json!({"t": "state", "s": {
                "cpu": "12.5",
                "ac_power": true,
                "session_locked": false,
                "volume": 40.0,
                "keep_awake": true,
                "csb_game": false,
                "cs_build": "OFF",
                "screen_time_left": null,
                "screen_time_limit": 90.0,
            }})
        );
    }

    #[test]
    fn a_number_that_does_not_parse_stays_text() {
        let frame = state_frame(&values(&[("volume", "loud")]));
        assert_eq!(frame["s"]["volume"], "loud");
    }

    #[test]
    fn info_frame_says_what_the_client_is() {
        assert_eq!(
            info_frame(ClientRole::App, ClientPlatform::Windows, &[]),
            json!({
                "t": "info",
                "role": "app",
                "platform": "windows",
                "app": env!("CARGO_PKG_VERSION"),
            })
        );
        let service = info_frame(ClientRole::Service, ClientPlatform::Windows, &[]);
        assert_eq!(service["role"], "service");
        assert!(service.get("features").is_none());
        let daemon = info_frame(ClientRole::Daemon, ClientPlatform::Linux, &[]);
        assert_eq!(daemon["role"], "daemon");
        assert_eq!(daemon["platform"], "linux");
    }

    #[test]
    fn info_frame_lists_the_features_it_is_given() {
        let frame = info_frame(
            ClientRole::App,
            ClientPlatform::Windows,
            &[feature::MEDIA_PLAYER, feature::NOTIFY_TAG],
        );
        assert_eq!(frame["features"], json!(["media_player", "notify_tag"]));
        // The integration accepts the names only in the form of a cap.
        assert!([feature::MEDIA_PLAYER, feature::NOTIFY_TAG]
            .iter()
            .all(|name| is_cap(name)));
    }

    /// The integration ignores an `info` whose version it cannot read: three
    /// numbers, then at most 20 characters after `-` or `+`.
    #[test]
    fn info_frame_carries_a_version_the_integration_accepts() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(is_version(version));
        let suffix = version.find(['-', '+']).map(|at| &version[at + 1..]);
        assert!(suffix.is_none_or(|suffix| suffix.len() <= 20));
    }

    #[test]
    fn reads_the_answer_of_home_assistant() {
        let info =
            parse_info(&json!({"t": "info", "integration": "0.9.0", "caps": ["info"]})).unwrap();
        assert_eq!(info.integration, "0.9.0");
        assert_eq!(info.caps, ["info"]);
        assert!(info.has_cap("info"));
        assert!(!info.has_cap("bye"));

        for version in ["10.20.30", "0.9.0-beta.1", "1.2.3+build-7"] {
            let frame = json!({"integration": version, "caps": []});
            assert_eq!(parse_info(&frame).unwrap().integration, version);
        }
        let caps: Vec<String> = (0..32).map(|n| format!("cap_{n}")).collect();
        let frame = json!({"integration": "0.9.0", "caps": caps, "later": {"any": "thing"}});
        assert_eq!(parse_info(&frame).unwrap().caps.len(), 32);
    }

    #[test]
    fn ignores_an_answer_with_a_bad_version() {
        let long = format!("1.2.3-{}", "a".repeat(35));
        assert_eq!(long.len(), 41);
        let versions = [
            json!(null),
            json!(9),
            json!(["0.9.0"]),
            json!(""),
            json!("0.9"),
            json!("0.9.0.1"),
            json!("v0.9.0"),
            json!("0.9.0-"),
            json!("0.9.0-beta+1"),
            json!("0.9.0 beta"),
            json!("0..0"),
            json!(" 0.9.0"),
            json!("0.9.0\n"),
            json!("0.9.0-[x](http://example.com)"),
            json!("\u{0660}.\u{0669}.\u{0660}"),
            json!(long),
        ];
        for version in versions {
            let frame = json!({"integration": version, "caps": ["info"]});
            assert_eq!(parse_info(&frame), None, "{version}");
        }
        assert_eq!(parse_info(&json!({"caps": ["info"]})), None);
    }

    #[test]
    fn ignores_an_answer_with_bad_caps() {
        let too_many: Vec<String> = (0..33).map(|n| format!("cap_{n}")).collect();
        let caps = [
            json!(null),
            json!("info"),
            json!({"info": true}),
            json!([1]),
            json!([""]),
            json!(["Info"]),
            json!(["two words"]),
            json!(["1st"]),
            json!(["info", "fs-2"]),
            json!(["a".repeat(33)]),
            json!(too_many),
        ];
        for caps in caps {
            let frame = json!({"integration": "0.9.0", "caps": caps});
            assert_eq!(parse_info(&frame), None, "{caps}");
        }
        assert_eq!(parse_info(&json!({"integration": "0.9.0"})), None);
        assert_eq!(parse_info(&json!("info")), None);
    }
}
