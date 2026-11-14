//! The machine scope: what a client reports and does without a signed-in
//! user. A Windows service or a daemon declares these entities with
//! `"scope": "machine"`; the desktop app declares the same ones as part of
//! its full list.
//!
//! Nothing is defined twice. The descriptors come from `SENSOR_DEFS`,
//! `COMMAND_DEFS` and the hardware probes, the values from the collector the
//! app also uses, and the frames from the encoders the app also uses.

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::config::AppConfig;
use crate::discovery;
use crate::sensors::{self, OwnedSensorDef, SensorDef, SENSOR_DEFS};
use crate::sys_commands::{self, CommandDef, COMMAND_DEFS};

/// Collects the values of the machine scope: the sensors in `SENSOR_KEYS` and
/// the hardware sensors detected on this computer.
pub use crate::sensors::MachineCollector as Collector;

/// Keys of the built-in sensors in the machine scope. The hardware sensors
/// (drives, disk activity, GPU, temperatures) are detected at run time, and
/// every one of them belongs to the scope as well.
pub const SENSOR_KEYS: [&str; 8] = [
    "cpu", "memory", "disk", "net_down", "net_up", "battery", "ac_power", "uptime",
];

/// Keys of the buttons in the machine scope: the power commands.
pub const COMMAND_KEYS: [&str; 4] = sys_commands::POWER_COMMANDS;

/// Descriptors of the built-in sensors in the machine scope.
pub fn sensor_defs() -> impl Iterator<Item = &'static SensorDef> {
    SENSOR_DEFS
        .iter()
        .filter(|def| SENSOR_KEYS.contains(&def.id))
}

/// Descriptors of the buttons in the machine scope.
pub fn command_defs() -> impl Iterator<Item = &'static CommandDef> {
    COMMAND_DEFS
        .iter()
        .filter(|def| COMMAND_KEYS.contains(&def.id))
}

/// The complete `declare` frame of the machine scope: the enabled sensors,
/// the enabled hardware sensors out of `hardware_defs` (the list that
/// `Collector::collect` returns) and the power buttons.
pub fn declare_frame(cfg: &AppConfig, hardware_defs: &[OwnedSensorDef]) -> Value {
    let mut entities: Vec<Value> = sensor_defs()
        .filter(|def| sensors::is_enabled(cfg, def.id))
        .map(discovery::link_sensor_entity)
        .collect();
    entities.extend(
        hardware_defs
            .iter()
            .filter(|def| sensors::is_hardware_enabled(cfg, &def.id))
            .map(discovery::link_hardware_entity),
    );
    entities.extend(command_defs().map(discovery::link_command_entity));
    json!({
        "t": "declare",
        "scope": "machine",
        "device": discovery::link_device(cfg),
        "entities": entities,
    })
}

/// The `state` frame for the values that `Collector::collect` returns.
pub fn state_frame(values: &HashMap<String, String>) -> Value {
    crate::link::state_frame(values)
}

/// Runs the command behind one of the buttons in `COMMAND_KEYS`. Every other
/// key is refused, the other commands of the app included.
pub fn run_command(key: &str) -> Result<(), String> {
    if !COMMAND_KEYS.contains(&key) {
        return Err(format!("not a machine command: {key}"));
    }
    sys_commands::run_power(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::SessionEntities;
    use crate::sys_commands::PRESENT_DEFS;

    fn hardware(id: &str, name: &str, unit: &str, icon: &str) -> OwnedSensorDef {
        OwnedSensorDef {
            id: id.into(),
            name: name.into(),
            component: "sensor".into(),
            unit: Some(unit.into()),
            device_class: None,
            icon: Some(icon.into()),
            privacy: false,
            default_enabled: true,
            hardware: true,
        }
    }

    /// Every hardware sensor the probes can report, as on a PC with one drive.
    fn detected_hardware() -> Vec<OwnedSensorDef> {
        vec![
            hardware(
                "cpu_temperature",
                "CPU temperature",
                "°C",
                "mdi:thermometer",
            ),
            hardware("disk_c_free", "Disk C free", "GiB", "mdi:harddisk"),
            hardware("disk_c_usage", "Disk C usage", "%", "mdi:harddisk"),
            hardware("disk_read", "Disk read", "MB/s", "mdi:harddisk-plus"),
            hardware("disk_write", "Disk write", "MB/s", "mdi:harddisk-plus"),
            hardware("gpu_memory", "GPU memory", "MiB", "mdi:memory"),
            hardware("gpu_memory_total", "GPU memory total", "MiB", "mdi:memory"),
            hardware(
                "gpu_temperature",
                "GPU temperature",
                "°C",
                "mdi:thermometer",
            ),
            hardware("gpu_usage", "GPU usage", "%", "mdi:expansion-card"),
        ]
    }

    fn entities(frame: &Value) -> &Vec<Value> {
        frame["entities"].as_array().unwrap()
    }

    fn keys(frame: &Value) -> Vec<&str> {
        entities(frame)
            .iter()
            .map(|entity| entity["key"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn machine_scope_lists_exactly_its_sensors_hardware_and_power_buttons() {
        let frame = declare_frame(&AppConfig::default(), &detected_hardware());
        assert_eq!(frame["t"], "declare");
        assert_eq!(frame["scope"], "machine");
        assert_eq!(
            keys(&frame),
            [
                "cpu",
                "memory",
                "disk",
                "net_down",
                "net_up",
                "battery",
                "ac_power",
                "uptime",
                "cpu_temperature",
                "disk_c_free",
                "disk_c_usage",
                "disk_read",
                "disk_write",
                "gpu_memory",
                "gpu_memory_total",
                "gpu_temperature",
                "gpu_usage",
                "sleep",
                "hibernate",
                "shutdown",
                "restart",
            ]
        );
    }

    #[test]
    fn every_machine_entity_is_in_the_app_declaration_with_the_same_descriptor() {
        let cfg = AppConfig {
            device_name: "Office PC".into(),
            ..AppConfig::default()
        };
        let detected = detected_hardware();
        let machine = declare_frame(&cfg, &detected);
        let app = discovery::build_link_declare(&cfg, &detected, &SessionEntities::default());

        assert_eq!(machine["device"], app["device"]);
        assert_eq!(machine["device"]["name"], "Office PC");
        assert!(app.get("scope").is_none(), "the app declares the full list");
        for entity in entities(&machine) {
            let twin = entities(&app)
                .iter()
                .find(|candidate| candidate["key"] == entity["key"]);
            assert_eq!(twin, Some(entity), "{}", entity["key"]);
        }
    }

    #[test]
    fn disabled_sensors_are_not_declared() {
        let mut cfg = AppConfig::default();
        cfg.sensors_enabled.insert("cpu".into(), false);
        cfg.sensors_enabled.insert("gpu_usage".into(), false);
        let frame = declare_frame(&cfg, &detected_hardware());
        let declared = keys(&frame);
        assert!(!declared.contains(&"cpu"));
        assert!(!declared.contains(&"gpu_usage"));
        assert!(declared.contains(&"memory"));
        assert!(declared.contains(&"shutdown"));
    }

    #[test]
    fn machine_keys_name_existing_public_descriptors() {
        let sensors: Vec<&SensorDef> = sensor_defs().collect();
        assert_eq!(sensors.len(), SENSOR_KEYS.len());
        assert!(
            sensors.iter().all(|def| !def.privacy),
            "nothing privacy-sensitive belongs to the machine scope"
        );
        assert_eq!(command_defs().count(), COMMAND_KEYS.len());
    }

    #[test]
    fn only_the_power_commands_run_in_the_machine_scope() {
        // The power commands themselves are not run here: they would turn the
        // computer off.
        let others = COMMAND_DEFS
            .iter()
            .chain(PRESENT_DEFS)
            .map(|def| def.id)
            .filter(|id| !COMMAND_KEYS.contains(id))
            .chain(["type_text", "open_url", "volume", "custom_backup", ""]);
        for key in others {
            assert!(run_command(key).is_err(), "{key}");
        }
    }

    #[test]
    fn state_frame_sends_machine_values_typed() {
        let values = HashMap::from([
            ("cpu".to_string(), "12.5".to_string()),
            ("ac_power".to_string(), "ON".to_string()),
            ("disk_c_free".to_string(), "120.50".to_string()),
        ]);
        assert_eq!(
            state_frame(&values),
            json!({"t": "state", "s": {"cpu": "12.5", "ac_power": true, "disk_c_free": "120.50"}})
        );
    }
}
