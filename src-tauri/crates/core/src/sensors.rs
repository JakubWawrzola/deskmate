//! The sensor descriptor shared with the app. The registry of built-in
//! sensors and the value collection live in the app's `sensors.rs`.

use serde::Serialize;

/// Serializable owned descriptor used for hardware sensors discovered at runtime.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OwnedSensorDef {
    pub id: String,
    pub name: String,
    pub component: String,
    pub unit: Option<String>,
    pub device_class: Option<String>,
    pub icon: Option<String>,
    pub privacy: bool,
    pub default_enabled: bool,
    /// Detected at runtime (drives, GPU, temperatures), listed on the Sensors
    /// page in their own panel.
    #[serde(default)]
    pub hardware: bool,
}
