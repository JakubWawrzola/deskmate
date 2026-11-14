//! Session frames that every client builds the same way (docs/LINK.md,
//! "Session frames"). The `declare` frame is in `discovery.rs` and
//! `machine.rs`.

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
                "keep_awake" | "audio_mute" | "mic_mute" | "family_rules" | "block_apps"
            ) =>
            {
                Value::Bool(value.eq_ignore_ascii_case("ON"))
            }
            // no daily limit: unknown in Home Assistant
            _ if key == "screen_time_left" && value == "None" => Value::Null,
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
}
