//! System commands executed on request from HA.
//! SECURITY: the MQTT payload is NEVER executed. We only run
//! predefined actions keyed by topic, plus custom commands stored in the config
//! (added deliberately by the user in the UI). The only payload-as-parameter case is
//! the volume number for `volume` (parsed, clamped 0-100).

use std::process::Command;
use url::Url;

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandDef {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
}

pub const COMMAND_DEFS: &[CommandDef] = &[
    CommandDef { id: "lock", name: "Lock", icon: "mdi:lock" },
    CommandDef { id: "sleep", name: "Sleep", icon: "mdi:power-sleep" },
    CommandDef { id: "hibernate", name: "Hibernate", icon: "mdi:power-sleep" },
    CommandDef { id: "shutdown", name: "Shutdown", icon: "mdi:power" },
    CommandDef { id: "restart", name: "Restart", icon: "mdi:restart" },
    CommandDef { id: "monitor_off", name: "Monitors off", icon: "mdi:monitor-off" },
    CommandDef { id: "media_play_pause", name: "Media play/pause", icon: "mdi:play-pause" },
    CommandDef { id: "media_next", name: "Media next", icon: "mdi:skip-next" },
    CommandDef { id: "media_prev", name: "Media previous", icon: "mdi:skip-previous" },
    CommandDef { id: "empty_recycle_bin", name: "Empty recycle bin", icon: "mdi:delete-empty" },
];

/// Presentation commands - shown in discovery ONLY when allow_input=true.
pub const PRESENT_DEFS: &[CommandDef] = &[
    CommandDef { id: "present_next", name: "Presentation next", icon: "mdi:arrow-right-bold" },
    CommandDef { id: "present_prev", name: "Presentation previous", icon: "mdi:arrow-left-bold" },
    CommandDef { id: "present_start", name: "Presentation start", icon: "mdi:play-box" },
    CommandDef { id: "present_black", name: "Presentation black", icon: "mdi:square" },
    CommandDef { id: "present_end", name: "Presentation end", icon: "mdi:stop" },
];

/// Runs a predefined action. Returns Err for an unknown key.
pub fn run_builtin(key: &str, payload: &str) -> Result<(), String> {
    match key {
        "lock" => lock(),
        "sleep" => spawn("rundll32", &["powrprof.dll,SetSuspendState", "0,1,0"]),
        "hibernate" => spawn("shutdown", &["/h"]),
        "shutdown" => spawn("shutdown", &["/s", "/t", "5"]),
        "restart" => spawn("shutdown", &["/r", "/t", "5"]),
        "monitor_off" => monitor_off(),
        "media_play_pause" => crate::media::control("play_pause"),
        "media_next" => crate::media::control("next"),
        "media_prev" => crate::media::control("prev"),
        "volume" => set_volume(payload),
        // text input + presentation (allow_input gate checked in MQTT routing)
        "type_text" => type_text(payload),
        "present_next" => tap_vk(0x27),   // VK_RIGHT
        "present_prev" => tap_vk(0x25),   // VK_LEFT
        "present_start" => tap_vk(0x74),  // VK_F5
        "present_black" => tap_vk(0x42),  // 'B'
        "present_end" => tap_vk(0x1B),    // VK_ESCAPE
        "empty_recycle_bin" => empty_recycle_bin(),
        // URL from HA (allow_input gate in MQTT routing); http/https ONLY
        "open_url" => open_url(payload),
        _ => Err(format!("unknown builtin command: {key}")),
    }
}

/// Parses a web URL accepted from MQTT. This deliberately does not decide which
/// hosts are trusted: blocking private addresses would break normal local HA
/// dashboards. Broker authentication/ACLs are the authorization boundary.
pub fn parse_web_url(input: &str) -> Result<Url, String> {
    let value = input.trim();
    if value.len() > 2048 || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("URL contains invalid characters".into());
    }
    let url = Url::parse(value).map_err(|_| "invalid URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
        return Err("only absolute http/https URLs are allowed".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with embedded credentials are not allowed".into());
    }
    Ok(url)
}

/// Normalizes an exact HTTP(S) origin used by the URL allowlist.
pub fn normalize_allowed_origin(input: &str) -> Result<String, String> {
    let url = parse_web_url(input)?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err("allowed URL entries must be origins without a path, query or fragment".into());
    }
    Ok(url.origin().ascii_serialization())
}

pub fn parse_allowed_web_url(
    cfg: &crate::config::AppConfig,
    input: &str,
) -> Result<Url, String> {
    let url = parse_web_url(input)?;
    let origin = url.origin().ascii_serialization();
    let explicitly_allowed = cfg
        .allowed_url_origins
        .iter()
        .filter_map(|entry| normalize_allowed_origin(entry).ok())
        .any(|allowed| allowed == origin);
    let ha_allowed = [&cfg.ha_url, &cfg.ha_url_remote]
        .into_iter()
        .filter(|entry| !entry.trim().is_empty())
        .filter_map(|entry| parse_web_url(entry).ok())
        .any(|entry| entry.origin().ascii_serialization() == origin);
    if !explicitly_allowed && !ha_allowed {
        return Err(format!("URL origin is not allowed: {origin}"));
    }
    Ok(url)
}

/// Opens a validated URL in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    let parsed = parse_web_url(url).map_err(|e| format!("open_url: {e}"))?;
    let u = parsed.as_str();
    #[cfg(windows)]
    {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide: Vec<u16> = u.encode_utf16().chain(std::iter::once(0)).collect();
        let r = unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
        };
        // ShellExecuteW returns HINSTANCE > 32 on success
        if r.0 as usize > 32 {
            Ok(())
        } else {
            Err(format!("ShellExecuteW failed ({})", r.0 as usize))
        }
    }
    #[cfg(not(windows))]
    {
        Err("windows only".into())
    }
}

pub fn open_allowed_url(cfg: &crate::config::AppConfig, input: &str) -> Result<(), String> {
    let url = parse_allowed_web_url(cfg, input).map_err(|e| format!("open_url: {e}"))?;
    open_url(url.as_str())
}

/// Empties the recycle bin (no confirmation dialog, sound, or progress bar).
#[cfg(windows)]
fn empty_recycle_bin() -> Result<(), String> {
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};
    let r = unsafe {
        SHEmptyRecycleBinW(None, windows::core::PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND)
    };
    // S_OK or the bin is already empty (E_UNEXPECTED is sometimes returned when empty) - both OK
    match r {
        Ok(()) => Ok(()),
        Err(e) if e.code().0 as u32 == 0x8000FFFF => Ok(()), // E_UNEXPECTED = empty bin
        Err(e) => Err(e.to_string()),
    }
}
#[cfg(not(windows))]
fn empty_recycle_bin() -> Result<(), String> {
    Err("windows only".into())
}

// ---------- keep awake (sleep prevention) ----------
// SetThreadExecutionState(ES_CONTINUOUS|...) applies per-THREAD, for as long as the
// thread lives - hence a dedicated thread with a channel (spawn_blocking would kill it).

/// Starts the keep-awake thread; returns a sender (true=block sleep, false=allow).
pub fn spawn_keep_awake() -> std::sync::mpsc::Sender<bool> {
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::{
                SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
            };
            while let Ok(on) = rx.recv() {
                unsafe {
                    if on {
                        SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);
                    } else {
                        SetThreadExecutionState(ES_CONTINUOUS);
                    }
                }
                log::info!("keep awake: {on}");
            }
        }
        #[cfg(not(windows))]
        {
            while rx.recv().is_ok() {}
        }
    });
    tx
}

/// Custom command from the config - run by ID (never content from MQTT).
/// `value` (MQTT payload: ON/OFF from a switch, a number from a number entity) is passed
/// to the script as $env:DESKMATE_VALUE - an environment variable, NOT interpolated into code.
pub fn run_custom(cfg: &crate::config::AppConfig, id: &str, value: &str) -> Result<(), String> {
    let cmd = cfg
        .custom_commands
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("unknown custom command: {id}"))?;
    if !cmd.enabled {
        crate::security::audit("custom_command", "blocked_disabled");
        return Err(format!("custom command '{id}' is disabled"));
    }
    if cmd.require_confirmation {
        if crate::sensors::session_locked() {
            crate::security::audit("custom_command", "blocked_locked");
            return Err("custom command confirmation unavailable while session is locked".into());
        }
        let name = crate::security::safe_preview(&cmd.name, 100);
        let approved = crate::security::confirm(
            "Deskmate custom command",
            &format!(
                "Home Assistant or a local Deskmate action requested the custom command:\n\n{name}\n\nRun it once?"
            ),
        );
        crate::security::audit(
            "custom_command_confirmation",
            if approved { "approved" } else { "denied" },
        );
        if !approved {
            return Err("custom command denied by user".into());
        }
    }
    // value is validated: only safe characters (digits/letters/./-/space/ON/OFF)
    let safe: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " .,-_".contains(*c))
        .take(64)
        .collect();
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd.command])
        .env("DESKMATE_VALUE", safe);
    // CREATE_NO_WINDOW - without this, the custom PS flashes a black console window
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    match command.spawn() {
        Ok(_) => {
            crate::security::audit("custom_command", "started");
            Ok(())
        }
        Err(e) => {
            crate::security::audit("custom_command", "failed");
            Err(e.to_string())
        }
    }
}

// ---------- key input (SendInput) ----------

#[cfg(windows)]
fn tap_vk(vk: u16) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };
    let mk = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [mk(KEYBD_EVENT_FLAGS(0)), mk(KEYEVENTF_KEYUP)];
    unsafe {
        let n = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if n == 0 {
            return Err("SendInput rejected (admin window / UIPI?)".into());
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn tap_vk(_vk: u16) -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(windows)]
fn type_text(text: &str) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        VIRTUAL_KEY,
    };
    let mut inputs: Vec<INPUT> = Vec::new();
    for unit in text.encode_utf16().take(2000) {
        for up in [false, true] {
            let mut flags = KEYEVENTF_UNICODE;
            if up {
                flags |= KEYEVENTF_KEYUP;
            }
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: unit,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }
    if inputs.is_empty() {
        return Ok(());
    }
    unsafe {
        let n = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if n == 0 {
            return Err("SendInput rejected (admin window / UIPI?)".into());
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn type_text(_text: &str) -> Result<(), String> {
    Err("windows only".into())
}

fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn lock() -> Result<(), String> {
    unsafe {
        windows::Win32::System::Shutdown::LockWorkStation()
            .map_err(|e| e.to_string())
    }
}
#[cfg(not(windows))]
fn lock() -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(windows)]
fn monitor_off() -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SYSCOMMAND};
    const SC_MONITORPOWER: usize = 0xF170;
    const HWND_BROADCAST: HWND = HWND(0xffff as _);
    unsafe {
        SendMessageW(
            HWND_BROADCAST,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_MONITORPOWER)),
            Some(LPARAM(2)),
        );
    }
    Ok(())
}
#[cfg(not(windows))]
fn monitor_off() -> Result<(), String> {
    Err("windows only".into())
}

// ---------- volume (WASAPI IAudioEndpointVolume) ----------

#[cfg(windows)]
fn with_endpoint<T>(f: impl FnOnce(&windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume) -> windows::core::Result<T>) -> Result<T, String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    unsafe {
        // COM per-thread; re-initialization returns S_FALSE - we ignore it
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| e.to_string())?;
        let vol: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())?;
        f(&vol).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
pub fn get_volume() -> Option<u8> {
    with_endpoint(|v| unsafe { v.GetMasterVolumeLevelScalar() })
        .ok()
        .map(|f| (f * 100.0).round().clamp(0.0, 100.0) as u8)
}
#[cfg(not(windows))]
pub fn get_volume() -> Option<u8> {
    None
}

#[cfg(windows)]
fn set_volume(payload: &str) -> Result<(), String> {
    let pct: f32 = payload.trim().parse::<f32>().map_err(|_| "volume: not a number".to_string())?;
    let scalar = (pct / 100.0).clamp(0.0, 1.0);
    with_endpoint(|v| unsafe { v.SetMasterVolumeLevelScalar(scalar, std::ptr::null()) })
}
#[cfg(not(windows))]
fn set_volume(_payload: &str) -> Result<(), String> {
    Err("windows only".into())
}
