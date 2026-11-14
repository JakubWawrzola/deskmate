//! Entity declarations of the app: the builders from deskmate-core, given what
//! only the signed-in user's session knows (sound devices, family rules).

use deskmate_core::discovery::{DiscoveryMsg, SessionEntities};
use serde_json::Value;

use crate::config::AppConfig;
use crate::sensors::OwnedSensorDef;

pub use deskmate_core::discovery::{
    link_hotkey_key, remove_custom, remove_custom_sensor, remove_hardware, remove_hotkey,
};

fn session_entities(cfg: &AppConfig) -> SessionEntities {
    SessionEntities {
        select_options: [crate::audio::OUTPUT_KEY, crate::audio::INPUT_KEY]
            .into_iter()
            .map(|key| (key.to_string(), crate::audio::options_for(key)))
            .collect(),
        family: crate::family::link_entities(cfg),
        family_keys: crate::family::ENTITIES,
    }
}

/// Full set of MQTT discovery messages derived from the config.
pub fn build_all(cfg: &AppConfig, hardware_defs: &[OwnedSensorDef]) -> Vec<DiscoveryMsg> {
    deskmate_core::discovery::build_all(cfg, hardware_defs, &session_entities(cfg))
}

/// Dynamic entity declaration for the Deskmate Link integration.
pub fn build_link_declare(cfg: &AppConfig, hardware_defs: &[OwnedSensorDef]) -> Value {
    deskmate_core::discovery::build_link_declare(cfg, hardware_defs, &session_entities(cfg))
}
