//! Komendy systemowe wykonywane na zadanie HA.
//! BEZPIECZENSTWO: payload MQTT NIGDY nie jest wykonywany. Wykonujemy wylacznie
//! predefiniowane akcje po kluczu topicu oraz custom-komendy zapisane w configu
//! (dodane swiadomie przez uzytkownika w UI). Jedynym payloadem-parametrem jest
//! liczba glosnosci dla `volume` (parsowana, clamp 0-100).

use std::process::Command;

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
];

/// Komendy prezentacji - pokazywane w discovery TYLKO gdy allow_input=true.
pub const PRESENT_DEFS: &[CommandDef] = &[
    CommandDef { id: "present_next", name: "Presentation next", icon: "mdi:arrow-right-bold" },
    CommandDef { id: "present_prev", name: "Presentation previous", icon: "mdi:arrow-left-bold" },
    CommandDef { id: "present_start", name: "Presentation start", icon: "mdi:play-box" },
    CommandDef { id: "present_black", name: "Presentation black", icon: "mdi:square" },
    CommandDef { id: "present_end", name: "Presentation end", icon: "mdi:stop" },
];

/// Wykonuje predefiniowana akcje. Zwraca Err dla nieznanego klucza.
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
        // wpis tekstu + prezentacja (gate allow_input sprawdzany w routingu MQTT)
        "type_text" => type_text(payload),
        "present_next" => tap_vk(0x27),   // VK_RIGHT
        "present_prev" => tap_vk(0x25),   // VK_LEFT
        "present_start" => tap_vk(0x74),  // VK_F5
        "present_black" => tap_vk(0x42),  // 'B'
        "present_end" => tap_vk(0x1B),    // VK_ESCAPE
        _ => Err(format!("unknown builtin command: {key}")),
    }
}

/// Custom komenda z configu - wykonywana po ID (nigdy tresc z MQTT).
/// `value` (payload MQTT: ON/OFF ze switcha, liczba z number) trafia do skryptu
/// jako $env:DESKMATE_VALUE - zmienna srodowiskowa, NIE interpolacja do kodu.
pub fn run_custom(cfg: &crate::config::AppConfig, id: &str, value: &str) -> Result<(), String> {
    let cmd = cfg
        .custom_commands
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("unknown custom command: {id}"))?;
    // wartosc walidowana: tylko bezpieczne znaki (cyfry/liter/./-/spacja/ON/OFF)
    let safe: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " .,-_".contains(*c))
        .take(64)
        .collect();
    Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd.command])
        .env("DESKMATE_VALUE", safe)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---------- wpis klawiszy (SendInput) ----------

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
            return Err("SendInput odrzucone (okno admin / UIPI?)".into());
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
            return Err("SendInput odrzucone (okno admin / UIPI?)".into());
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

// ---------- glosnosc (WASAPI IAudioEndpointVolume) ----------

#[cfg(windows)]
fn with_endpoint<T>(f: impl FnOnce(&windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume) -> windows::core::Result<T>) -> Result<T, String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    unsafe {
        // COM per-thread; powtorna inicjalizacja zwraca S_FALSE - ignorujemy
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
