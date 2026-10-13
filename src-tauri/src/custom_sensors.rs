//! Sensors the user adds on the Sensors page (GitHub issue #5, HASS.Agent
//! users asked for them):
//! - process: is a program running (on/off);
//! - window: is there a visible window whose title contains a text (on/off);
//! - service: state of a Windows service (running, stopped, ...);
//! - powershell: the first line a PowerShell command prints, on its own
//!   interval (30 s or more) and in the background, so a slow script never
//!   delays the other sensors. Starts disabled, like custom commands.
//!
//! On/off sensors use keys "csb_<id>", the others "cs_<id>", so the Link
//! encoder can send the on/off ones as booleans without looking up the config.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::config::{AppConfig, CustomSensor};

pub const KINDS: [&str; 4] = ["process", "window", "service", "powershell"];
const POWERSHELL_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_VALUE_CHARS: usize = 255;

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

/// Values of every enabled custom sensor for this publish interval.
pub fn collect(cfg: &AppConfig, out: &mut HashMap<String, String>) {
    let enabled: Vec<&CustomSensor> = cfg.custom_sensors.iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() {
        return;
    }
    let processes = enabled
        .iter()
        .any(|s| s.kind == "process")
        .then(crate::processes::snapshot)
        .unwrap_or_default();
    let titles = enabled
        .iter()
        .any(|s| s.kind == "window")
        .then(win::visible_window_titles)
        .unwrap_or_default();
    for sensor in enabled {
        let value = match sensor.kind.as_str() {
            "process" => Some(on_off(crate::processes::is_running(
                &processes,
                &sensor.target,
            ))),
            "window" => {
                let needle = sensor.target.trim().to_lowercase();
                Some(on_off(
                    !needle.is_empty() && titles.iter().any(|title| title.contains(&needle)),
                ))
            }
            "service" => Some(win::service_state(sensor.target.trim())),
            "powershell" => powershell_value(sensor),
            _ => None,
        };
        if let Some(value) = value {
            out.insert(key(sensor), value);
        }
    }
}

fn on_off(on: bool) -> String {
    if on { "ON" } else { "OFF" }.to_string()
}

// ---------- PowerShell sensors: background runs, cached result ----------

#[derive(Default)]
struct Run {
    started: Option<Instant>,
    running: bool,
    value: Option<String>,
}

fn runs() -> &'static Mutex<HashMap<String, Run>> {
    static RUNS: OnceLock<Mutex<HashMap<String, Run>>> = OnceLock::new();
    RUNS.get_or_init(Default::default)
}

/// Last result, and a new background run when the interval has passed.
fn powershell_value(sensor: &CustomSensor) -> Option<String> {
    let interval = Duration::from_secs(sensor.interval_secs.max(30));
    let mut table = runs().lock().unwrap_or_else(|e| e.into_inner());
    // keyed by id and command, so an edited command runs again right away
    let slot = format!("{}\u{1f}{}", sensor.id, sensor.target);
    let run = table.entry(slot.clone()).or_default();
    let due = run.started.is_none_or(|t| t.elapsed() >= interval);
    if due && !run.running {
        run.running = true;
        run.started = Some(Instant::now());
        let command = sensor.target.clone();
        std::thread::spawn(move || {
            let value = run_powershell(&command);
            let mut table = runs().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(run) = table.get_mut(&slot) {
                run.running = false;
                if value.is_some() {
                    run.value = value;
                }
            }
        });
    }
    run.value.clone()
}

/// First non-empty line of stdout, at most 255 characters. None on failure or
/// after the timeout, so a broken script keeps the last good value.
fn run_powershell(command: &str) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-Command",
        command,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + POWERSHELL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                crate::security::audit("custom_sensor", "powershell_timeout");
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_VALUE_CHARS).collect())
}

#[cfg(windows)]
mod win {
    use windows::core::{BOOL, HSTRING};
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT,
        SERVICE_PAUSED, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_PENDING,
        SERVICE_STATUS, SERVICE_STOPPED, SERVICE_STOP_PENDING,
    };
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, IsWindowVisible};

    /// running | stopped | starting | stopping | paused | not found
    pub fn service_state(name: &str) -> String {
        if name.is_empty() {
            return "not found".into();
        }
        unsafe {
            let Ok(manager) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else {
                return "unknown".into();
            };
            let state = match OpenServiceW(manager, &HSTRING::from(name), SERVICE_QUERY_STATUS) {
                Ok(service) => {
                    let mut status = SERVICE_STATUS::default();
                    let state = if QueryServiceStatus(service, &mut status).is_ok() {
                        match status.dwCurrentState {
                            SERVICE_RUNNING => "running",
                            SERVICE_STOPPED => "stopped",
                            SERVICE_START_PENDING => "starting",
                            SERVICE_STOP_PENDING => "stopping",
                            SERVICE_PAUSED => "paused",
                            _ => "unknown",
                        }
                    } else {
                        "unknown"
                    };
                    let _ = CloseServiceHandle(service);
                    state
                }
                Err(_) => "not found",
            };
            let _ = CloseServiceHandle(manager);
            state.into()
        }
    }

    /// Titles of visible top-level windows, in lower case.
    pub fn visible_window_titles() -> Vec<String> {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let titles = &mut *(lparam.0 as *mut Vec<String>);
            if IsWindowVisible(hwnd).as_bool() {
                let mut buffer = [0u16; 512];
                let len = GetWindowTextW(hwnd, &mut buffer);
                if len > 0 {
                    titles.push(String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase());
                }
            }
            BOOL(1)
        }
        let mut titles: Vec<String> = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(collect),
                LPARAM(&mut titles as *mut Vec<String> as isize),
            );
        }
        titles
    }
}

#[cfg(not(windows))]
mod win {
    pub fn service_state(_name: &str) -> String {
        "unknown".into()
    }
    pub fn visible_window_titles() -> Vec<String> {
        Vec::new()
    }
}
