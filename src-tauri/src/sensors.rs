//! System sensors. Definition registry + value collection.
//! Sensors marked privacy=true are DISABLED BY DEFAULT (opt-in in the UI).

use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{Disks, Networks, System};

#[derive(Debug, Clone, Serialize)]
pub struct SensorDef {
    pub id: &'static str,
    pub name: &'static str,
    /// HA component: sensor | binary_sensor
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

/// Collector state kept between ticks (network counters).
pub struct Collector {
    sys: System,
    networks: Networks,
    disks: Disks,
    hardware: crate::hardware::HardwareCollector,
    last_tick: std::time::Instant,
    net_primed: bool,
    last_clipboard_fingerprint: Option<u64>,
    approved_clipboard_fingerprint: Option<u64>,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            hardware: crate::hardware::HardwareCollector::new(),
            last_tick: std::time::Instant::now(),
            net_primed: false,
            last_clipboard_fingerprint: None,
            approved_clipboard_fingerprint: None,
        }
    }

    /// Collects values for ENABLED sensors. Key = sensor id, value = MQTT payload.
    pub fn collect(
        &mut self,
        cfg: &crate::config::AppConfig,
        mqtt_connected: bool,
    ) -> (HashMap<String, String>, Vec<OwnedSensorDef>) {
        let mut out = HashMap::new();
        let elapsed = self.last_tick.elapsed().as_secs_f64().max(0.5);
        self.last_tick = std::time::Instant::now();

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
            let sysdrive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
            for d in self.disks.list() {
                let mp = d.mount_point().to_string_lossy().to_string();
                if mp.starts_with(&sysdrive) {
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
        if is_enabled(cfg, "current_user") {
            out.insert(
                "current_user".into(),
                std::env::var("USERNAME").unwrap_or_else(|_| "?".into()),
            );
        }

        #[cfg(windows)]
        {
            if is_enabled(cfg, "battery") || is_enabled(cfg, "ac_power") {
                if let Some((pct, ac)) = win::battery() {
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
            if is_enabled(cfg, "idle") {
                out.insert("idle".into(), win::idle_seconds().to_string());
            }
            if is_enabled(cfg, "session_locked") {
                out.insert(
                    "session_locked".into(),
                    if win::is_locked() { "ON" } else { "OFF" }.into(),
                );
            }
            if is_enabled(cfg, "active_window") {
                out.insert("active_window".into(), truncate(&win::active_window(), 250));
            }
            if is_enabled(cfg, "wifi_ssid") {
                out.insert(
                    "wifi_ssid".into(),
                    win::wifi_ssid().unwrap_or_else(|| "not connected".into()),
                );
            }
            if mqtt_connected {
                self.collect_clipboard(cfg, &mut out);
            }
            if is_enabled(cfg, "camera_in_use") {
                out.insert(
                    "camera_in_use".into(),
                    if win::capability_in_use("webcam") {
                        "ON"
                    } else {
                        "OFF"
                    }
                    .into(),
                );
            }
            if is_enabled(cfg, "mic_in_use") {
                out.insert(
                    "mic_in_use".into(),
                    if win::capability_in_use("microphone") {
                        "ON"
                    } else {
                        "OFF"
                    }
                    .into(),
                );
            }
            if is_enabled(cfg, "volume") {
                if let Some(v) = crate::sys_commands::get_volume() {
                    out.insert("volume".into(), v.to_string());
                }
            }
            // media: a single SMTC query for all 4 sensors
            let media_on = ["media_title", "media_artist", "media_app", "media_status"]
                .iter()
                .any(|id| is_enabled(cfg, id));
            if media_on {
                let info = crate::media::current();
                let (t, a, app, st) = match info {
                    Some(m) => (m.title, m.artist, m.app, m.status),
                    None => ("none".into(), "none".into(), "none".into(), "idle".into()),
                };
                if is_enabled(cfg, "media_title") {
                    out.insert("media_title".into(), truncate(&t, 250));
                }
                if is_enabled(cfg, "media_artist") {
                    out.insert("media_artist".into(), truncate(&a, 250));
                }
                if is_enabled(cfg, "media_app") {
                    out.insert("media_app".into(), app);
                }
                if is_enabled(cfg, "media_status") {
                    out.insert("media_status".into(), st);
                }
            }
        }

        let (hardware_values, hardware_defs) = self.hardware.collect(&self.disks, elapsed);
        for (id, value) in hardware_values {
            if is_hardware_enabled(cfg, &id) {
                out.insert(id, value);
            }
        }

        (out, hardware_defs)
    }

    #[cfg(windows)]
    fn collect_clipboard(
        &mut self,
        cfg: &crate::config::AppConfig,
        out: &mut HashMap<String, String>,
    ) {
        use std::hash::{DefaultHasher, Hash, Hasher};

        let mode = cfg.clipboard_read_mode.as_str();
        if mode == "off" || win::is_locked() {
            self.last_clipboard_fingerprint = None;
            self.approved_clipboard_fingerprint = None;
            return;
        }
        let Some(content) = crate::clipboard::get_text() else {
            return;
        };
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let fingerprint = hasher.finish();

        if mode == "automatic" {
            if self.last_clipboard_fingerprint != Some(fingerprint) {
                crate::security::audit("clipboard_read", "automatic");
            }
            self.last_clipboard_fingerprint = Some(fingerprint);
            out.insert("clipboard".into(), truncate(&content, 240));
            return;
        }

        if mode != "confirm" {
            return;
        }
        if self.last_clipboard_fingerprint != Some(fingerprint) {
            self.last_clipboard_fingerprint = Some(fingerprint);
            let approved = crate::security::confirm(
                "Deskmate clipboard access",
                &format!(
                    "Home Assistant wants to receive the current clipboard ({} characters).\n\nAllow this value to be published until the clipboard changes?",
                    content.chars().count()
                ),
            );
            self.approved_clipboard_fingerprint = approved.then_some(fingerprint);
            crate::security::audit(
                "clipboard_read_confirmation",
                if approved { "approved" } else { "denied" },
            );
        }
        if self.approved_clipboard_fingerprint == Some(fingerprint) {
            out.insert("clipboard".into(), truncate(&content, 240));
        }
    }
}

#[cfg(windows)]
pub fn session_locked() -> bool {
    win::is_locked()
}

#[cfg(not(windows))]
pub fn session_locked() -> bool {
    false
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.into()
    } else {
        s.chars().take(n).collect()
    }
}

#[cfg(windows)]
pub mod win {
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS,
    };
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    /// (percent 0-100 or None if no battery, whether plugged into power)
    pub fn battery() -> Option<(Option<u8>, bool)> {
        unsafe {
            let mut st = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut st).is_err() {
                return None;
            }
            let pct = if st.BatteryLifePercent == 255 {
                None
            } else {
                Some(st.BatteryLifePercent)
            };
            Some((pct, st.ACLineStatus == 1))
        }
    }

    pub fn idle_seconds() -> u64 {
        unsafe {
            let mut lii = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if GetLastInputInfo(&mut lii).as_bool() {
                let now = GetTickCount();
                (now.wrapping_sub(lii.dwTime) / 1000) as u64
            } else {
                0
            }
        }
    }

    /// Session locked = cannot open the input desktop with SWITCHDESKTOP rights.
    pub fn is_locked() -> bool {
        unsafe {
            const DESKTOP_SWITCHDESKTOP: DESKTOP_ACCESS_FLAGS = DESKTOP_ACCESS_FLAGS(0x0100);
            match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) {
                Ok(h) => {
                    let _ = CloseDesktop(h);
                    false
                }
                Err(_) => true,
            }
        }
    }

    /// "Window title (process.exe)"
    pub fn active_window() -> String {
        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.0.is_null() {
                return "none".into();
            }
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let proc_name = if pid != 0 {
                match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    Ok(h) => {
                        let mut pbuf = [0u16; 512];
                        let mut plen = pbuf.len() as u32;
                        let name = if QueryFullProcessImageNameW(
                            h,
                            PROCESS_NAME_FORMAT(0),
                            windows::core::PWSTR(pbuf.as_mut_ptr()),
                            &mut plen,
                        )
                        .is_ok()
                        {
                            let full = String::from_utf16_lossy(&pbuf[..plen as usize]);
                            full.rsplit('\\').next().unwrap_or("").to_string()
                        } else {
                            String::new()
                        };
                        let _ = CloseHandle(h);
                        name
                    }
                    Err(_) => String::new(),
                }
            } else {
                String::new()
            };
            if title.is_empty() && proc_name.is_empty() {
                "none".into()
            } else if proc_name.is_empty() {
                title
            } else {
                format!("{} ({})", title, proc_name)
            }
        }
    }

    /// Whether some application is using the camera/microphone: CapabilityAccessManager
    /// ConsentStore - LastUsedTimeStop == 0 means "currently in use".
    /// capability: "webcam" | "microphone".
    pub fn capability_in_use(capability: &str) -> bool {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let base = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{capability}"
        );
        let Ok(root) = hkcu.open_subkey(&base) else {
            return false;
        };
        // packaged apps = direct subkeys; desktop apps = NonPackaged\*
        let check = |key: &RegKey| -> bool {
            key.enum_keys().flatten().any(|name| {
                key.open_subkey(&name)
                    .ok()
                    .and_then(|sub| sub.get_value::<u64, _>("LastUsedTimeStop").ok())
                    .map(|stop| stop == 0)
                    .unwrap_or(false)
            })
        };
        if check(&root) {
            return true;
        }
        root.open_subkey("NonPackaged")
            .map(|np| check(&np))
            .unwrap_or(false)
    }

    /// SSID via netsh (no stable API in the windows crate without wlansvc COM)
    pub fn wifi_ssid() -> Option<String> {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW - without this, netsh flashes a black cmd window every interval
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        for line in text.lines() {
            let l = line.trim();
            // first line "SSID" (not BSSID)
            if l.starts_with("SSID") && !l.starts_with("BSSID") {
                return l
                    .split(':')
                    .nth(1)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
        }
        None
    }
}
