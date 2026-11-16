//! Entity declarations of the app: the builders from deskmate-core, given what
//! only the signed-in user's session knows (sound devices, family rules).

use deskmate_core::discovery::{DiscoveryMsg, SessionEntities};
use deskmate_core::link::media::declares_media_player;
use deskmate_core::link::HubInfo;
use serde_json::Value;

use crate::config::AppConfig;
use crate::sensors::OwnedSensorDef;

pub use deskmate_core::discovery::{
    link_hotkey_key, remove_custom, remove_custom_sensor, remove_hardware, remove_hotkey,
};

/// `hub` is what Home Assistant said about itself in the current Link
/// session, None over MQTT and before it has answered.
fn session_entities(cfg: &AppConfig, hub: Option<&HubInfo>) -> SessionEntities {
    SessionEntities {
        select_options: [crate::audio::OUTPUT_KEY, crate::audio::INPUT_KEY]
            .into_iter()
            .map(|key| (key.to_string(), crate::audio::options_for(key)))
            .collect(),
        #[cfg(windows)]
        family: crate::family::link_entities(cfg),
        #[cfg(windows)]
        family_keys: crate::family::ENTITIES,
        #[cfg(not(windows))]
        family: Vec::new(),
        #[cfg(not(windows))]
        family_keys: &[],
        media_player: declares_media_player(cfg, hub),
        #[cfg(windows)]
        brightness: crate::monitor::brightness::is_supported(),
        // No brightness control in the Linux preview (linux/entities.rs).
        #[cfg(not(windows))]
        brightness: false,
    }
}

/// Full set of MQTT discovery messages derived from the config.
pub fn build_all(cfg: &AppConfig, hardware_defs: &[OwnedSensorDef]) -> Vec<DiscoveryMsg> {
    let messages =
        deskmate_core::discovery::build_all(cfg, hardware_defs, &session_entities(cfg, None));
    // Linux declares only what works there (linux/entities.rs).
    #[cfg(target_os = "linux")]
    let messages = crate::linux::entities::filter_discovery(messages, hardware_defs);
    messages
}

/// Dynamic entity declaration for the Deskmate Link integration. The media
/// player is in it only once `hub` has announced the entity kind.
pub fn build_link_declare(
    cfg: &AppConfig,
    hardware_defs: &[OwnedSensorDef],
    hub: Option<&HubInfo>,
) -> Value {
    let declare = deskmate_core::discovery::build_link_declare(
        cfg,
        hardware_defs,
        &session_entities(cfg, hub),
    );
    #[cfg(target_os = "linux")]
    let declare = crate::linux::entities::filter_declare(declare, hardware_defs);
    declare
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

    fn declares_media(cfg: &AppConfig, hub: Option<&HubInfo>) -> bool {
        build_link_declare(cfg, &[], hub)["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entity| entity["key"] == "media")
    }

    /// An integration that does not announce the media player never gets
    /// one, and neither does a user who left the media status sensor off.
    #[test]
    fn the_media_player_needs_the_cap_and_the_media_status_sensor() {
        let mut cfg = AppConfig::default();
        let new_hub = hub(&["info", "scoped_declare", "bye", "media_player"]);
        let old_hub = hub(&["info", "scoped_declare", "bye"]);
        assert!(!declares_media(&cfg, Some(&new_hub)));

        cfg.sensors_enabled.insert("media_title".into(), true);
        assert!(!declares_media(&cfg, Some(&new_hub)));
        cfg.sensors_enabled.insert("media_status".into(), true);
        assert!(declares_media(&cfg, Some(&new_hub)));
        assert!(!declares_media(&cfg, Some(&old_hub)));
        assert!(!declares_media(&cfg, None));
        assert!(build_all(&cfg, &[])
            .iter()
            .all(|(topic, _)| !topic.contains("media_player")));
    }

    /// A service declares the machine scope for the same computer, so Home
    /// Assistant must get identical descriptors from both.
    #[test]
    fn app_declaration_contains_the_machine_scope_unchanged() {
        let cfg = AppConfig::default();
        let app = build_link_declare(&cfg, &[], None);
        let machine = deskmate_core::machine::declare_frame(&cfg, &[]);
        let app_entities = app["entities"].as_array().unwrap();
        let machine_entities = machine["entities"].as_array().unwrap();

        assert_eq!(app["device"], machine["device"]);
        assert!(!machine_entities.is_empty());
        for entity in machine_entities {
            let twin = app_entities
                .iter()
                .find(|candidate| candidate["key"] == entity["key"]);
            assert_eq!(twin, Some(entity), "{}", entity["key"]);
        }
    }
}
