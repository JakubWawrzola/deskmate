//! Which entities the Linux preview declares. Home Assistant gets an entity
//! only when it works on this computer: the builders of deskmate-core
//! describe the whole Windows app, and their output passes through
//! `filter_declare` and `filter_discovery` here. Anything not named below,
//! including whatever a later version adds, stays out until it is made to
//! work on Linux.

use serde_json::Value;
use std::collections::HashSet;

use crate::sensors::OwnedSensorDef;
use crate::sys_commands::CommandDef;
use deskmate_core::discovery::DiscoveryMsg;

/// The machine scope that every Linux computer has.
const ALWAYS: [&str; 10] = [
    "cpu",
    "memory",
    "disk",
    "net_down",
    "net_up",
    "uptime",
    "current_user",
    "sleep",
    "shutdown",
    "restart",
];
/// Through logind's session.
const SESSION: [&str; 4] = ["idle", "last_active", "session_locked", "lock"];
/// Through MPRIS; `media` is the media player entity, which the core
/// declares only when Home Assistant supports it.
const MEDIA: [&str; 8] = [
    deskmate_core::link::media::MEDIA_KEY,
    "media_title",
    "media_artist",
    "media_app",
    "media_status",
    "media_play_pause",
    "media_next",
    "media_prev",
];
/// Through wpctl or pactl.
const VOLUME: [&str; 2] = ["volume", "audio_mute"];
/// Through xdg-open; the core declares it only with *Allow input* on.
const OPEN_URL: &str = "open_url";
/// The prefix of the hotkey event entities (`link_hotkey_key`).
const HOTKEY_PREFIX: &str = "hotkey_";

/// What this computer offers right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offered {
    machine: deskmate_core::machine::Offered,
    session: bool,
    media: bool,
    volume: bool,
    open_url: bool,
    hotkeys: bool,
}

impl Offered {
    pub fn detect() -> Self {
        let capabilities = crate::platform::capabilities();
        Self {
            machine: deskmate_core::machine::Offered::detect(),
            session: capabilities.lock,
            media: capabilities.media,
            volume: capabilities.volume,
            open_url: capabilities.open_url,
            hotkeys: capabilities.global_shortcuts,
        }
    }

    /// Whether the entity `key` works here. `hardware` holds the ids of the
    /// hardware sensors found on this computer.
    pub fn offers(&self, key: &str, hardware: &HashSet<&str>) -> bool {
        match key {
            "battery" => self.machine.battery,
            "ac_power" => self.machine.ac_power,
            "hibernate" => self.machine.hibernate,
            OPEN_URL => self.open_url,
            _ if ALWAYS.contains(&key) => true,
            _ if SESSION.contains(&key) => self.session,
            _ if MEDIA.contains(&key) => self.media,
            _ if VOLUME.contains(&key) => self.volume,
            _ if key.starts_with(HOTKEY_PREFIX) => self.hotkeys,
            _ => hardware.contains(key),
        }
    }
}

fn hardware_ids(hardware_defs: &[OwnedSensorDef]) -> HashSet<&str> {
    hardware_defs.iter().map(|def| def.id.as_str()).collect()
}

/// The Link `declare` frame without what does not work here.
pub fn filter_declare(mut declare: Value, hardware_defs: &[OwnedSensorDef]) -> Value {
    filter_declare_for(
        &mut declare,
        &Offered::detect(),
        &hardware_ids(hardware_defs),
    );
    declare
}

fn filter_declare_for(declare: &mut Value, offered: &Offered, hardware: &HashSet<&str>) {
    if let Some(entities) = declare.get_mut("entities").and_then(Value::as_array_mut) {
        entities.retain(|entity| {
            entity
                .get("key")
                .and_then(Value::as_str)
                .is_some_and(|key| offered.offers(key, hardware))
        });
    }
}

/// The MQTT discovery messages, with an empty payload (which removes the
/// entity in Home Assistant) for what does not work here.
pub fn filter_discovery(
    messages: Vec<DiscoveryMsg>,
    hardware_defs: &[OwnedSensorDef],
) -> Vec<DiscoveryMsg> {
    filter_discovery_for(messages, &Offered::detect(), &hardware_ids(hardware_defs))
}

fn filter_discovery_for(
    messages: Vec<DiscoveryMsg>,
    offered: &Offered,
    hardware: &HashSet<&str>,
) -> Vec<DiscoveryMsg> {
    messages
        .into_iter()
        .map(|(topic, payload)| {
            let keep = object_id(&topic).is_some_and(|key| offered.offers(key, hardware));
            if keep {
                (topic, payload)
            } else {
                (topic, String::new())
            }
        })
        .collect()
}

/// `homeassistant/<component>/<node>/<object id>/config` -> the object id,
/// which is the entity's key.
fn object_id(topic: &str) -> Option<&str> {
    let mut parts = topic.rsplit('/');
    (parts.next()? == "config").then_some(())?;
    parts.next()
}

/// Whether a command from Home Assistant may run here: only one of an
/// entity this computer declares.
pub fn accepts_command(key: &str) -> bool {
    !key.starts_with(HOTKEY_PREFIX) && Offered::detect().offers(key, &HashSet::new())
}

/// The built-in sensors and buttons for the window's lists.
pub fn for_window(
    sensor_defs: Vec<OwnedSensorDef>,
    command_defs: Vec<CommandDef>,
    hardware_defs: &[OwnedSensorDef],
) -> (Vec<OwnedSensorDef>, Vec<CommandDef>) {
    let offered = Offered::detect();
    let hardware = hardware_ids(hardware_defs);
    (
        sensor_defs
            .into_iter()
            .filter(|def| offered.offers(&def.id, &hardware))
            .collect(),
        command_defs
            .into_iter()
            .filter(|def| offered.offers(def.id, &hardware))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use deskmate_core::discovery::link_hotkey_key;
    use serde_json::json;

    fn everything() -> Offered {
        Offered {
            machine: deskmate_core::machine::Offered::ALL,
            session: true,
            media: true,
            volume: true,
            open_url: true,
            hotkeys: true,
        }
    }

    fn nothing_optional() -> Offered {
        Offered {
            machine: deskmate_core::machine::Offered::NONE,
            session: false,
            media: false,
            volume: false,
            open_url: false,
            hotkeys: false,
        }
    }

    fn keys(declare: &Value) -> Vec<String> {
        declare["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entity| entity["key"].as_str().unwrap().to_string())
            .collect()
    }

    /// Everything switched on, so that the full Windows list is built.
    fn full_config() -> AppConfig {
        let mut cfg = AppConfig {
            allow_input: true,
            tts_enabled: true,
            clipboard_read_mode: "automatic".into(),
            clipboard_write_mode: "automatic".into(),
            ..Default::default()
        };
        for def in deskmate_core::sensors::SENSOR_DEFS {
            cfg.sensors_enabled.insert(def.id.to_string(), true);
        }
        // An MQTT device trigger: the hotkey kind that MQTT discovery declares.
        cfg.hotkeys.push(crate::config::Hotkey {
            id: "desk".into(),
            action: crate::config::ActionSpec {
                kind: "mqtt".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        cfg
    }

    #[test]
    fn declares_only_what_works_on_linux() {
        let cfg = full_config();
        let hardware = HashSet::from(["disk_root_free"]);
        let mut declare = deskmate_core::discovery::build_link_declare(
            &cfg,
            &[],
            &deskmate_core::discovery::SessionEntities::default(),
        );
        declare["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key": "disk_root_free", "kind": "sensor"}));
        filter_declare_for(&mut declare, &everything(), &hardware);
        let kept = keys(&declare);
        for key in [
            "cpu",
            "battery",
            "idle",
            "session_locked",
            "lock",
            "hibernate",
            "media_title",
            "media_play_pause",
            "volume",
            "audio_mute",
            "open_url",
            "disk_root_free",
        ] {
            assert!(kept.iter().any(|k| k == key), "{key} is declared");
        }
        assert!(kept.contains(&link_hotkey_key("desk")));
        assert!(link_hotkey_key("desk").starts_with(HOTKEY_PREFIX));
        for key in [
            "type_text",
            "present_next",
            "tts_say",
            "clipboard",
            "clipboard_set",
            "keep_awake",
            "mic_mute",
            "mic_volume",
            "audio_output",
            "sign_out",
            "monitor_off",
            "empty_recycle_bin",
            "active_window",
            "wifi_ssid",
            "camera_in_use",
            "presenting",
        ] {
            assert!(!kept.iter().any(|k| k == key), "{key} is not declared");
        }
    }

    #[test]
    fn a_computer_without_the_optional_parts_declares_the_machine_scope() {
        let mut declare = deskmate_core::discovery::build_link_declare(
            &full_config(),
            &[],
            &deskmate_core::discovery::SessionEntities::default(),
        );
        filter_declare_for(&mut declare, &nothing_optional(), &HashSet::new());
        let mut kept = keys(&declare);
        kept.sort();
        let mut expected: Vec<String> = ALWAYS.iter().map(|key| key.to_string()).collect();
        expected.sort();
        assert_eq!(kept, expected);
    }

    #[test]
    fn mqtt_discovery_removes_what_does_not_work() {
        let cfg = full_config();
        let messages = deskmate_core::discovery::build_all(
            &cfg,
            &[],
            &deskmate_core::discovery::SessionEntities::default(),
        );
        let filtered = filter_discovery_for(messages, &everything(), &HashSet::new());
        let payload = |object: &str| {
            filtered
                .iter()
                .find(|(topic, _)| topic.ends_with(&format!("/{object}/config")))
                .map(|(_, payload)| payload.clone())
        };
        assert!(!payload("cpu").unwrap().is_empty());
        assert!(!payload("lock").unwrap().is_empty());
        assert_eq!(payload("type_text").as_deref(), Some(""));
        assert_eq!(payload("keep_awake").as_deref(), Some(""));
        assert_eq!(payload("sign_out").as_deref(), Some(""));
        assert!(!payload("hotkey_desk").unwrap().is_empty());
    }

    #[test]
    fn reads_the_object_id_of_a_discovery_topic() {
        assert_eq!(
            object_id("homeassistant/sensor/my_pc/cpu/config"),
            Some("cpu")
        );
        assert_eq!(object_id("homeassistant/sensor/my_pc/cpu/state"), None);
        assert_eq!(object_id("config"), None);
    }

    #[test]
    fn commands_run_only_for_declared_entities() {
        let offered = nothing_optional();
        assert!(offered.offers("shutdown", &HashSet::new()));
        assert!(!offered.offers("lock", &HashSet::new()));
        assert!(!offered.offers("custom_backup", &HashSet::new()));
        assert!(!offered.offers("keep_awake", &HashSet::new()));
        assert!(!offered.offers("tts_say", &HashSet::new()));
        assert!(!offered.offers("brightness", &HashSet::new()));
        assert!(!offered.offers("wake_screen", &HashSet::new()));
        assert!(!offered.offers("user_state", &HashSet::new()));
        assert!(!offered.offers("screen_on", &HashSet::new()));
        assert!(!offered.offers("media", &HashSet::new()), "without MPRIS");
        assert!(everything().offers("media", &HashSet::new()));
    }
}
