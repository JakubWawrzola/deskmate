//! System sensors: the definition registry and the collection of the values
//! that need no signed-in user. The app adds the sensors that read the user's
//! session on top (its own `sensors.rs`).
//! Sensors marked privacy=true are DISABLED BY DEFAULT (opt-in in the UI).

use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{Disks, Networks, System};

#[derive(Debug, Clone, Serialize)]
pub struct SensorDef {
    pub id: &'static str,
    pub name: &'static str,
    /// HA component: sensor | binary_sensor | number | select
    pub component: &'static str,
    pub unit: Option<&'static str>,
    pub device_class: Option<&'static str>,
    pub icon: Option<&'static str>,
    /// true = privacy-sensitive, OFF by default
    pub privacy: bool,
    pub default_enabled: bool,
}

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

impl From<&SensorDef> for OwnedSensorDef {
    fn from(def: &SensorDef) -> Self {
        Self {
            id: def.id.into(),
            name: def.name.into(),
            component: def.component.into(),
            unit: def.unit.map(str::to_string),
            device_class: def.device_class.map(str::to_string),
            icon: def.icon.map(str::to_string),
            privacy: def.privacy,
            default_enabled: def.default_enabled,
            hardware: false,
        }
    }
}

pub fn static_defs() -> Vec<OwnedSensorDef> {
    SENSOR_DEFS.iter().map(OwnedSensorDef::from).collect()
}

pub const SENSOR_DEFS: &[SensorDef] = &[
    SensorDef {
        id: "cpu",
        name: "CPU usage",
        component: "sensor",
        unit: Some("%"),
        device_class: None,
        icon: Some("mdi:cpu-64-bit"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "memory",
        name: "Memory usage",
        component: "sensor",
        unit: Some("%"),
        device_class: None,
        icon: Some("mdi:memory"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "disk",
        name: "Disk usage",
        component: "sensor",
        unit: Some("%"),
        device_class: None,
        icon: Some("mdi:harddisk"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "net_down",
        name: "Network down",
        component: "sensor",
        unit: Some("kB/s"),
        device_class: None,
        icon: Some("mdi:download-network"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "net_up",
        name: "Network up",
        component: "sensor",
        unit: Some("kB/s"),
        device_class: None,
        icon: Some("mdi:upload-network"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "battery",
        name: "Battery",
        component: "sensor",
        unit: Some("%"),
        device_class: Some("battery"),
        icon: None,
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "ac_power",
        name: "Plugged in",
        component: "binary_sensor",
        unit: None,
        device_class: Some("plug"),
        icon: None,
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "uptime",
        name: "Uptime",
        component: "sensor",
        unit: Some("s"),
        device_class: Some("duration"),
        icon: Some("mdi:clock-outline"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "idle",
        name: "Idle time",
        component: "sensor",
        unit: Some("s"),
        device_class: Some("duration"),
        icon: Some("mdi:sleep"),
        privacy: false,
        default_enabled: true,
    },
    // HASS.Agent's LastActive, used for presence (GitHub issue #5). A
    // timestamp needs the 0.8.0 integration on Link, hence off by default.
    SensorDef {
        id: "last_active",
        name: "Last active",
        component: "sensor",
        unit: None,
        device_class: Some("timestamp"),
        icon: Some("mdi:account-clock"),
        privacy: false,
        default_enabled: false,
    },
    // no device_class "lock": in HA, lock means on=unlocked (the opposite of ours)
    SensorDef {
        id: "session_locked",
        name: "Session locked",
        component: "binary_sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:monitor-lock"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "current_user",
        name: "Current user",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:account"),
        privacy: false,
        default_enabled: true,
    },
    // Windows' own "should I stay quiet" signal: a full-screen app, a game, a
    // presentation or Do Not Disturb. Useful as a condition for automations that
    // would otherwise interrupt at the worst possible moment.
    SensorDef {
        id: "presenting",
        name: "Presenting or full screen",
        component: "binary_sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:presentation"),
        privacy: false,
        default_enabled: true,
    },
    // The same Windows signal as "presenting", told apart: normal,
    // fullscreen, game, presentation or busy (the app's user_state.rs).
    SensorDef {
        id: "user_state",
        name: "User state",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:account-eye"),
        privacy: false,
        default_enabled: true,
    },
    // --- privacy-sensitive: OPT-IN ---
    SensorDef {
        id: "active_window",
        name: "Active window",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:window-restore"),
        privacy: true,
        default_enabled: false,
    },
    SensorDef {
        id: "wifi_ssid",
        name: "WiFi SSID",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:wifi"),
        privacy: true,
        default_enabled: false,
    },
    SensorDef {
        id: "media_title",
        name: "Media title",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:music-note"),
        privacy: true,
        default_enabled: false,
    },
    SensorDef {
        id: "media_artist",
        name: "Media artist",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:account-music"),
        privacy: true,
        default_enabled: false,
    },
    SensorDef {
        id: "media_app",
        name: "Media app",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:application"),
        privacy: true,
        default_enabled: false,
    },
    SensorDef {
        id: "media_status",
        name: "Media status",
        component: "sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:play-pause"),
        privacy: true,
        default_enabled: false,
    },
    SensorDef {
        id: "camera_in_use",
        name: "Camera in use",
        component: "binary_sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:camera"),
        privacy: true,
        default_enabled: false,
    },
    SensorDef {
        id: "mic_in_use",
        name: "Microphone in use",
        component: "binary_sensor",
        unit: None,
        device_class: None,
        icon: Some("mdi:microphone"),
        privacy: true,
        default_enabled: false,
    },
    // volume is published as a number entity state (see discovery)
    SensorDef {
        id: "volume",
        name: "Volume",
        component: "number",
        unit: Some("%"),
        device_class: None,
        icon: Some("mdi:volume-high"),
        privacy: false,
        default_enabled: true,
    },
    // Microphone level for the default recording device (audio.rs)
    SensorDef {
        id: "mic_volume",
        name: "Microphone volume",
        component: "number",
        unit: Some("%"),
        device_class: None,
        icon: Some("mdi:microphone"),
        privacy: false,
        default_enabled: true,
    },
    // Default sound devices as select entities; the options are the devices
    // Windows reports (audio.rs)
    SensorDef {
        id: "audio_output",
        name: "Sound output",
        component: "select",
        unit: None,
        device_class: None,
        icon: Some("mdi:speaker"),
        privacy: false,
        default_enabled: true,
    },
    SensorDef {
        id: "audio_input",
        name: "Sound input",
        component: "select",
        unit: None,
        device_class: None,
        icon: Some("mdi:microphone-variant"),
        privacy: false,
        default_enabled: true,
    },
];

pub fn is_enabled(cfg: &crate::config::AppConfig, id: &str) -> bool {
    let def = SENSOR_DEFS.iter().find(|d| d.id == id);
    match cfg.sensors_enabled.get(id) {
        Some(v) => *v,
        None => def.map(|d| d.default_enabled).unwrap_or(false),
    }
}

pub fn is_hardware_enabled(cfg: &crate::config::AppConfig, id: &str) -> bool {
    cfg.sensors_enabled.get(id).copied().unwrap_or(true)
}

/// Collects the sensors that need no signed-in user: CPU, memory, the system
/// drive, network, uptime, power and the detected hardware. Keeps the state
/// needed between ticks (network counters).
pub struct MachineCollector {
    sys: System,
    networks: Networks,
    disks: Disks,
    hardware: crate::hardware::HardwareCollector,
    last_tick: std::time::Instant,
    net_primed: bool,
}

impl MachineCollector {
    #[allow(
        clippy::new_without_default,
        reason = "new() lists the network adapters and drives and opens the hardware probes; Default would hide that work"
    )]
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            hardware: crate::hardware::HardwareCollector::new(),
            last_tick: std::time::Instant::now(),
            net_primed: false,
        }
    }

    /// Collects values for ENABLED sensors. Key = sensor id, value = payload.
    /// The second item lists the hardware sensors detected on this machine.
    pub fn collect(
        &mut self,
        cfg: &crate::config::AppConfig,
    ) -> (HashMap<String, String>, Vec<OwnedSensorDef>) {
        let mut out = HashMap::new();
        let elapsed = self.last_tick.elapsed().as_secs_f64().max(0.5);
        self.last_tick = std::time::Instant::now();

        self.collect_builtin(cfg, elapsed, &mut out);

        let (hardware_values, hardware_defs) = self.hardware.collect(&self.disks, elapsed);
        for (id, value) in hardware_values {
            if is_hardware_enabled(cfg, &id) {
                out.insert(id, value);
            }
        }

        (out, hardware_defs)
    }

    /// The sensors from `SENSOR_DEFS` that this collector covers.
    fn collect_builtin(
        &mut self,
        cfg: &crate::config::AppConfig,
        elapsed: f64,
        out: &mut HashMap<String, String>,
    ) {
        if is_enabled(cfg, "cpu") {
            self.sys.refresh_cpu_usage();
            out.insert("cpu".into(), format!("{:.1}", self.sys.global_cpu_usage()));
        }
        if is_enabled(cfg, "memory") {
            self.sys.refresh_memory();
            let total = self.sys.total_memory().max(1);
            out.insert(
                "memory".into(),
                format!(
                    "{:.1}",
                    self.sys.used_memory() as f64 / total as f64 * 100.0
                ),
            );
        }
        self.disks.refresh(true);
        if is_enabled(cfg, "disk") {
            // system drive (usually C:)
            #[cfg(windows)]
            let sysdrive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
            for d in self.disks.list() {
                #[cfg(windows)]
                let is_system_drive = d
                    .mount_point()
                    .to_string_lossy()
                    .starts_with(sysdrive.as_str());
                #[cfg(target_os = "linux")]
                let is_system_drive =
                    d.mount_point() == std::path::Path::new(crate::linux::disks::ROOT_MOUNT);
                if is_system_drive {
                    let total = d.total_space().max(1);
                    let used = total - d.available_space();
                    out.insert(
                        "disk".into(),
                        format!("{:.1}", used as f64 / total as f64 * 100.0),
                    );
                    break;
                }
            }
        }
        if is_enabled(cfg, "net_down") || is_enabled(cfg, "net_up") {
            self.networks.refresh(true);
            let (mut rx, mut tx) = (0u64, 0u64);
            for (_name, data) in self.networks.iter() {
                // Linux counts what programs on this computer send each
                // other; that is not network traffic.
                #[cfg(target_os = "linux")]
                if _name == LOOPBACK_INTERFACE {
                    continue;
                }
                rx += data.received();
                tx += data.transmitted();
            }
            if !self.net_primed {
                // the first reading is a baseline (delta since process start) - it would
                // be an absurd spike; we publish 0 and prime the counter.
                self.net_primed = true;
                if is_enabled(cfg, "net_down") {
                    out.insert("net_down".into(), "0.0".into());
                }
                if is_enabled(cfg, "net_up") {
                    out.insert("net_up".into(), "0.0".into());
                }
            } else {
                if is_enabled(cfg, "net_down") {
                    out.insert(
                        "net_down".into(),
                        format!("{:.1}", rx as f64 / elapsed / 1024.0),
                    );
                }
                if is_enabled(cfg, "net_up") {
                    out.insert(
                        "net_up".into(),
                        format!("{:.1}", tx as f64 / elapsed / 1024.0),
                    );
                }
            }
        }
        if is_enabled(cfg, "uptime") {
            out.insert("uptime".into(), System::uptime().to_string());
        }

        if is_enabled(cfg, "battery") || is_enabled(cfg, "ac_power") {
            if let Some((pct, ac)) = battery() {
                if is_enabled(cfg, "battery") {
                    if let Some(p) = pct {
                        out.insert("battery".into(), p.to_string());
                    }
                }
                if is_enabled(cfg, "ac_power") {
                    out.insert("ac_power".into(), if ac { "ON" } else { "OFF" }.into());
                }
            }
        }
    }
}

/// (percent 0-100 or None if no battery, whether plugged into power)
#[cfg(windows)]
fn battery() -> Option<(Option<u8>, bool)> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut st = SYSTEM_POWER_STATUS::default();
    // SAFETY: `st` is a valid, writable SYSTEM_POWER_STATUS for the duration
    // of the call, which only fills it in.
    if unsafe { GetSystemPowerStatus(&mut st) }.is_err() {
        return None;
    }
    let pct = if st.BatteryLifePercent == 255 {
        None
    } else {
        Some(st.BatteryLifePercent)
    };
    Some((pct, st.ACLineStatus == 1))
}

/// The same pair from the power supplies in sysfs. None on a computer with
/// neither a battery nor a power adapter to ask.
#[cfg(target_os = "linux")]
fn battery() -> Option<(Option<u8>, bool)> {
    let status = crate::linux::power_supply::read(std::path::Path::new(crate::linux::SYS));
    Some((status.battery_percent, status.plugged_in?))
}

/// The name of the loopback interface on Linux.
#[cfg(target_os = "linux")]
const LOOPBACK_INTERFACE: &str = "lo";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    /// The machine scope is declared from `machine::SENSOR_KEYS`. A built-in
    /// sensor collected here but missing there would be sent without ever
    /// being declared.
    #[test]
    fn machine_collector_reports_only_sensors_of_the_machine_scope() {
        let mut collector = MachineCollector::new();
        let mut values = HashMap::new();
        collector.collect_builtin(&AppConfig::default(), 1.0, &mut values);
        for key in values.keys() {
            assert!(
                crate::machine::SENSOR_KEYS.contains(&key.as_str()),
                "{key} is collected but not in the machine scope"
            );
        }
        for key in ["cpu", "memory", "net_down", "net_up", "uptime"] {
            assert!(values.contains_key(key), "{key} was not collected");
        }
    }

    #[test]
    fn a_disabled_sensor_is_not_collected() {
        let mut cfg = AppConfig::default();
        cfg.sensors_enabled.insert("uptime".into(), false);
        let mut collector = MachineCollector::new();
        let mut values = HashMap::new();
        collector.collect_builtin(&cfg, 1.0, &mut values);
        assert!(!values.contains_key("uptime"));
        assert!(values.contains_key("cpu"));
    }
}
