//! Sensory systemowe. Rejestr definicji + zbieranie wartosci.
//! Sensory oznaczone privacy=true sa DOMYSLNIE WYLACZONE (opt-in w UI).

use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{Disks, Networks, System};

#[derive(Debug, Clone, Serialize)]
pub struct SensorDef {
    pub id: &'static str,
    pub name: &'static str,
    /// komponent HA: sensor | binary_sensor
    pub component: &'static str,
    pub unit: Option<&'static str>,
    pub device_class: Option<&'static str>,
    pub icon: Option<&'static str>,
    /// true = wrazliwy prywatnosciowo, domyslnie OFF
    pub privacy: bool,
    pub default_enabled: bool,
}

pub const SENSOR_DEFS: &[SensorDef] = &[
    SensorDef { id: "cpu", name: "CPU usage", component: "sensor", unit: Some("%"), device_class: None, icon: Some("mdi:cpu-64-bit"), privacy: false, default_enabled: true },
    SensorDef { id: "memory", name: "Memory usage", component: "sensor", unit: Some("%"), device_class: None, icon: Some("mdi:memory"), privacy: false, default_enabled: true },
    SensorDef { id: "disk", name: "Disk usage", component: "sensor", unit: Some("%"), device_class: None, icon: Some("mdi:harddisk"), privacy: false, default_enabled: true },
    SensorDef { id: "net_down", name: "Network down", component: "sensor", unit: Some("kB/s"), device_class: None, icon: Some("mdi:download-network"), privacy: false, default_enabled: true },
    SensorDef { id: "net_up", name: "Network up", component: "sensor", unit: Some("kB/s"), device_class: None, icon: Some("mdi:upload-network"), privacy: false, default_enabled: true },
    SensorDef { id: "battery", name: "Battery", component: "sensor", unit: Some("%"), device_class: Some("battery"), icon: None, privacy: false, default_enabled: true },
    SensorDef { id: "ac_power", name: "Plugged in", component: "binary_sensor", unit: None, device_class: Some("plug"), icon: None, privacy: false, default_enabled: true },
    SensorDef { id: "uptime", name: "Uptime", component: "sensor", unit: Some("s"), device_class: Some("duration"), icon: Some("mdi:clock-outline"), privacy: false, default_enabled: true },
    SensorDef { id: "idle", name: "Idle time", component: "sensor", unit: Some("s"), device_class: Some("duration"), icon: Some("mdi:sleep"), privacy: false, default_enabled: true },
    // bez device_class "lock": w HA lock znaczy on=odblokowany (odwrotnie niz my)
    SensorDef { id: "session_locked", name: "Session locked", component: "binary_sensor", unit: None, device_class: None, icon: Some("mdi:monitor-lock"), privacy: false, default_enabled: true },
    SensorDef { id: "current_user", name: "Current user", component: "sensor", unit: None, device_class: None, icon: Some("mdi:account"), privacy: false, default_enabled: true },
    // --- privacy-sensitive: OPT-IN ---
    SensorDef { id: "active_window", name: "Active window", component: "sensor", unit: None, device_class: None, icon: Some("mdi:window-restore"), privacy: true, default_enabled: false },
    SensorDef { id: "wifi_ssid", name: "WiFi SSID", component: "sensor", unit: None, device_class: None, icon: Some("mdi:wifi"), privacy: true, default_enabled: false },
    SensorDef { id: "media_title", name: "Media title", component: "sensor", unit: None, device_class: None, icon: Some("mdi:music-note"), privacy: true, default_enabled: false },
    SensorDef { id: "media_artist", name: "Media artist", component: "sensor", unit: None, device_class: None, icon: Some("mdi:account-music"), privacy: true, default_enabled: false },
    SensorDef { id: "media_app", name: "Media app", component: "sensor", unit: None, device_class: None, icon: Some("mdi:application"), privacy: true, default_enabled: false },
    SensorDef { id: "media_status", name: "Media status", component: "sensor", unit: None, device_class: None, icon: Some("mdi:play-pause"), privacy: true, default_enabled: false },
    SensorDef { id: "clipboard", name: "Clipboard", component: "sensor", unit: None, device_class: None, icon: Some("mdi:clipboard-text"), privacy: true, default_enabled: false },
    // volume publikowany jako stan encji number (patrz discovery)
    SensorDef { id: "volume", name: "Volume", component: "number", unit: Some("%"), device_class: None, icon: Some("mdi:volume-high"), privacy: false, default_enabled: true },
];

pub fn is_enabled(cfg: &crate::config::AppConfig, id: &str) -> bool {
    let def = SENSOR_DEFS.iter().find(|d| d.id == id);
    match cfg.sensors_enabled.get(id) {
        Some(v) => *v,
        None => def.map(|d| d.default_enabled).unwrap_or(false),
    }
}

/// Stan kolektora trzymany miedzy tickami (liczniki sieci).
pub struct Collector {
    sys: System,
    networks: Networks,
    disks: Disks,
    last_tick: std::time::Instant,
    net_primed: bool,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            last_tick: std::time::Instant::now(),
            net_primed: false,
        }
    }

    /// Zbiera wartosci WLACZONYCH sensorow. Klucz = id sensora, wartosc = payload MQTT.
    pub fn collect(&mut self, cfg: &crate::config::AppConfig) -> HashMap<String, String> {
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
            out.insert("memory".into(), format!("{:.1}", self.sys.used_memory() as f64 / total as f64 * 100.0));
        }
        if is_enabled(cfg, "disk") {
            self.disks.refresh(true);
            // dysk systemowy (zwykle C:)
            let sysdrive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
            for d in self.disks.list() {
                let mp = d.mount_point().to_string_lossy().to_string();
                if mp.starts_with(&sysdrive) {
                    let total = d.total_space().max(1);
                    let used = total - d.available_space();
                    out.insert("disk".into(), format!("{:.1}", used as f64 / total as f64 * 100.0));
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
                // pierwszy odczyt to baseline (delta od startu procesu) - byłby
                // absurdalnym spike'iem; publikujemy 0 i primujemy licznik.
                self.net_primed = true;
                if is_enabled(cfg, "net_down") { out.insert("net_down".into(), "0.0".into()); }
                if is_enabled(cfg, "net_up") { out.insert("net_up".into(), "0.0".into()); }
            } else {
                if is_enabled(cfg, "net_down") {
                    out.insert("net_down".into(), format!("{:.1}", rx as f64 / elapsed / 1024.0));
                }
                if is_enabled(cfg, "net_up") {
                    out.insert("net_up".into(), format!("{:.1}", tx as f64 / elapsed / 1024.0));
                }
            }
        }
        if is_enabled(cfg, "uptime") {
            out.insert("uptime".into(), System::uptime().to_string());
        }
        if is_enabled(cfg, "current_user") {
            out.insert("current_user".into(), std::env::var("USERNAME").unwrap_or_else(|_| "?".into()));
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
                out.insert("session_locked".into(), if win::is_locked() { "ON" } else { "OFF" }.into());
            }
            if is_enabled(cfg, "active_window") {
                out.insert("active_window".into(), truncate(&win::active_window(), 250));
            }
            if is_enabled(cfg, "wifi_ssid") {
                out.insert("wifi_ssid".into(), win::wifi_ssid().unwrap_or_else(|| "not connected".into()));
            }
            if is_enabled(cfg, "clipboard") {
                if let Some(c) = crate::clipboard::get_text() {
                    out.insert("clipboard".into(), truncate(&c, 240));
                }
            }
            if is_enabled(cfg, "volume") {
                if let Some(v) = crate::sys_commands::get_volume() {
                    out.insert("volume".into(), v.to_string());
                }
            }
            // media: jedna proba SMTC dla wszystkich 4 sensorow
            let media_on = ["media_title", "media_artist", "media_app", "media_status"]
                .iter()
                .any(|id| is_enabled(cfg, id));
            if media_on {
                let info = crate::media::current();
                let (t, a, app, st) = match info {
                    Some(m) => (m.title, m.artist, m.app, m.status),
                    None => ("none".into(), "none".into(), "none".into(), "idle".into()),
                };
                if is_enabled(cfg, "media_title") { out.insert("media_title".into(), truncate(&t, 250)); }
                if is_enabled(cfg, "media_artist") { out.insert("media_artist".into(), truncate(&a, 250)); }
                if is_enabled(cfg, "media_app") { out.insert("media_app".into(), app); }
                if is_enabled(cfg, "media_status") { out.insert("media_status".into(), st); }
            }
        }

        out
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.into() } else { s.chars().take(n).collect() }
}

#[cfg(windows)]
pub mod win {
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS};
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

    /// (procent 0-100 lub None gdy brak baterii, czy podpiety do pradu)
    pub fn battery() -> Option<(Option<u8>, bool)> {
        unsafe {
            let mut st = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut st).is_err() {
                return None;
            }
            let pct = if st.BatteryLifePercent == 255 { None } else { Some(st.BatteryLifePercent) };
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

    /// Sesja zablokowana = nie mozna otworzyc input desktopu z prawem SWITCHDESKTOP.
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

    /// "Tytul okna (proces.exe)"
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
                        let name = if QueryFullProcessImageNameW(h, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(pbuf.as_mut_ptr()), &mut plen).is_ok() {
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

    /// SSID przez netsh (brak stabilnego API w windows crate bez wlansvc COM)
    pub fn wifi_ssid() -> Option<String> {
        let out = std::process::Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        for line in text.lines() {
            let l = line.trim();
            // pierwsza linia "SSID" (nie BSSID)
            if l.starts_with("SSID") && !l.starts_with("BSSID") {
                return l.split(':').nth(1).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            }
        }
        None
    }
}
