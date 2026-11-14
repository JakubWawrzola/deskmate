//! How a sensor the user added on the Sensors page is described to Home
//! Assistant. The values are collected by the app (its `custom_sensors.rs`).
//!
//! On/off sensors use keys "csb_<id>", the others "cs_<id>", so the Link
//! encoder can send the on/off ones as booleans without looking up the config.

use crate::config::CustomSensor;

pub fn is_binary(kind: &str) -> bool {
    matches!(kind, "process" | "window")
}

pub fn key(sensor: &CustomSensor) -> String {
    if is_binary(&sensor.kind) {
        format!("csb_{}", sensor.id)
    } else {
        format!("cs_{}", sensor.id)
    }
}

pub fn component(sensor: &CustomSensor) -> &'static str {
    if is_binary(&sensor.kind) {
        "binary_sensor"
    } else {
        "sensor"
    }
}

pub fn icon(sensor: &CustomSensor) -> &'static str {
    match sensor.kind.as_str() {
        "process" => "mdi:application-cog",
        "window" => "mdi:application-outline",
        "service" => "mdi:cog-outline",
        _ => "mdi:console",
    }
}
