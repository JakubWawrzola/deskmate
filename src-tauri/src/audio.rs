//! Microphone control and switching the default sound devices.
//!
//! Microphone mute and volume use Core Audio (IAudioEndpointVolume), like the
//! speaker controls in sys_commands.rs. The default device is changed through
//! IPolicyConfig, the interface the Windows sound settings use themselves. It
//! is not documented, but has been stable since Windows 7 and is what
//! SoundSwitch, EarTrumpet and HASS.Agent use.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    Output,
    Input,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    /// Friendly name, made unique ("Headset", "Headset (2)") so Home Assistant
    /// can select a device by name.
    pub name: String,
}

/// Device names last offered to Home Assistant: (outputs, inputs).
static OFFERED: Mutex<(Vec<String>, Vec<String>)> = Mutex::new((Vec::new(), Vec::new()));
/// Set when a device was plugged in or removed; the entities are declared again.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// Entity keys of the two device selects.
pub const OUTPUT_KEY: &str = "audio_output";
pub const INPUT_KEY: &str = "audio_input";

/// Names offered for a select entity (empty until the first sensor tick).
pub fn options_for(key: &str) -> Vec<String> {
    let offered = OFFERED.lock().map(|o| o.clone()).unwrap_or_default();
    match key {
        OUTPUT_KEY => offered.0,
        INPUT_KEY => offered.1,
        _ => Vec::new(),
    }
}

/// True once after the device list changed since the last call.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

fn remember(flow: Flow, names: Vec<String>) {
    if let Ok(mut offered) = OFFERED.lock() {
        let slot = match flow {
            Flow::Output => &mut offered.0,
            Flow::Input => &mut offered.1,
        };
        if *slot != names {
            *slot = names;
            CHANGED.store(true, Ordering::Relaxed);
        }
    }
}

/// Gives every device a unique name, numbering repeated ones.
fn unique_names(devices: Vec<(String, String)>) -> Vec<Device> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    devices
        .into_iter()
        .map(|(id, name)| {
            let count = seen.entry(name.to_lowercase()).or_insert(0);
            *count += 1;
            let name = if *count == 1 { name } else { format!("{name} ({count})") };
            Device { id, name }
        })
        .collect()
}

/// Sensor tick: microphone state and the current default devices.
pub fn collect(cfg: &crate::config::AppConfig, out: &mut HashMap<String, String>) {
    // Published unconditionally, like "Mute audio", so the switch follows
    // changes made on the computer itself.
    if let Some(muted) = mic_muted() {
        out.insert("mic_mute".into(), if muted { "ON" } else { "OFF" }.into());
    }
    if crate::sensors::is_enabled(cfg, "mic_volume") {
        if let Some(volume) = mic_volume() {
            out.insert("mic_volume".into(), volume.to_string());
        }
    }
    for (flow, key) in [(Flow::Output, OUTPUT_KEY), (Flow::Input, INPUT_KEY)] {
        if !crate::sensors::is_enabled(cfg, key) {
            continue;
        }
        let devices = devices(flow);
        remember(flow, devices.iter().map(|d| d.name.clone()).collect());
        if let Some(id) = default_id(flow) {
            if let Some(device) = devices.iter().find(|d| d.id == id) {
                out.insert(key.into(), device.name.clone());
            }
        }
    }
}

/// Selects the device Home Assistant picked by name as the default for every
/// role (apps, media, calls), like "Set as default device" in Windows.
pub fn select(key: &str, name: &str) -> Result<(), String> {
    let flow = match key {
        OUTPUT_KEY => Flow::Output,
        INPUT_KEY => Flow::Input,
        _ => return Err(format!("unknown sound device entity: {key}")),
    };
    let name = name.trim();
    let device = devices(flow)
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| format!("no sound device named {name}"))?;
    win::set_default(&device.id)
}

pub fn set_mic_volume(payload: &str) -> Result<(), String> {
    let percent: f32 = payload
        .trim()
        .parse()
        .map_err(|_| "microphone volume: not a number".to_string())?;
    win::set_capture_volume((percent / 100.0).clamp(0.0, 1.0))
}

pub fn set_mic_muted(muted: bool) -> Result<(), String> {
    win::set_capture_muted(muted)
}

pub fn mic_muted() -> Option<bool> {
    win::capture_muted()
}

pub fn mic_volume() -> Option<u8> {
    win::capture_volume().map(|v| (v * 100.0).round().clamp(0.0, 100.0) as u8)
}

fn devices(flow: Flow) -> Vec<Device> {
    unique_names(win::endpoints(flow))
}

fn default_id(flow: Flow) -> Option<String> {
    win::default_endpoint_id(flow)
}

#[cfg(windows)]
#[allow(non_snake_case)] // COM method names of IPolicyConfig
mod win {
    use super::Flow;
    use std::ffi::c_void;
    use windows::core::{GUID, HRESULT, HSTRING, PCWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDevice,
        IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
    };
    use windows_core::{IUnknown, IUnknown_Vtbl};

    /// PKEY_Device_FriendlyName ("Speakers (Realtek(R) Audio)").
    const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    /// CLSID of CPolicyConfigClient.
    const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// IPolicyConfig (Windows 7 and later). Only SetDefaultEndpoint is called;
    /// the methods before it are declared to keep the vtable order.
    #[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, period: *mut i64, minimum: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(&self, device: PCWSTR, store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetPropertyValue(&self, device: PCWSTR, store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    fn data_flow(flow: Flow) -> EDataFlow {
        match flow {
            Flow::Output => eRender,
            Flow::Input => eCapture,
        }
    }

    fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
        unsafe {
            // COM per thread; a second initialisation returns S_FALSE.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        }
    }

    fn device_id(device: &IMMDevice) -> Option<String> {
        unsafe {
            let raw = device.GetId().ok()?;
            let id = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as *const c_void));
            id
        }
    }

    fn friendly_name(device: &IMMDevice) -> Option<String> {
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let mut value = store.GetValue(&FRIENDLY_NAME).ok()?;
            let name = PropVariantToStringAlloc(&value).ok().map(|raw| {
                let name = raw.to_string().unwrap_or_default();
                CoTaskMemFree(Some(raw.0 as *const c_void));
                name
            });
            let _ = PropVariantClear(&mut value);
            name.filter(|n| !n.trim().is_empty())
        }
    }

    /// (id, friendly name) of every active device of this kind.
    pub fn endpoints(flow: Flow) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let Ok(enumerator) = enumerator() else {
            return out;
        };
        unsafe {
            let Ok(collection) = enumerator.EnumAudioEndpoints(data_flow(flow), DEVICE_STATE_ACTIVE) else {
                return out;
            };
            let count = collection.GetCount().unwrap_or(0);
            for index in 0..count {
                let Ok(device) = collection.Item(index) else {
                    continue;
                };
                if let (Some(id), Some(name)) = (device_id(&device), friendly_name(&device)) {
                    out.push((id, name));
                }
            }
        }
        out
    }

    pub fn default_endpoint_id(flow: Flow) -> Option<String> {
        let enumerator = enumerator().ok()?;
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(data_flow(flow), eConsole) }.ok()?;
        device_id(&device)
    }

    pub fn set_default(id: &str) -> Result<(), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let policy: IPolicyConfig = CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
                .map_err(|e| format!("cannot change the sound device: {e}"))?;
            let id = HSTRING::from(id);
            for role in [eConsole, eMultimedia, eCommunications] {
                policy
                    .SetDefaultEndpoint(PCWSTR(id.as_ptr()), role)
                    .ok()
                    .map_err(|e| format!("cannot change the sound device: {e}"))?;
            }
        }
        Ok(())
    }

    /// The default recording devices for apps and for calls. Usually one
    /// device; a headset can be the call device next to a webcam microphone,
    /// and both are muted together.
    fn capture_devices() -> Vec<IAudioEndpointVolume> {
        let Ok(enumerator) = enumerator() else {
            return Vec::new();
        };
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for role in [eConsole, eCommunications] {
            let Ok(device) = (unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, role) }) else {
                continue;
            };
            let id = device_id(&device);
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            if let Ok(volume) = unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) } {
                out.push(volume);
            }
        }
        out
    }

    pub fn capture_muted() -> Option<bool> {
        let first = capture_devices().into_iter().next()?;
        unsafe { first.GetMute() }.ok().map(|m| m.as_bool())
    }

    pub fn capture_volume() -> Option<f32> {
        let first = capture_devices().into_iter().next()?;
        unsafe { first.GetMasterVolumeLevelScalar() }.ok()
    }

    pub fn set_capture_muted(muted: bool) -> Result<(), String> {
        let devices = capture_devices();
        if devices.is_empty() {
            return Err("no microphone found".into());
        }
        for device in devices {
            unsafe { device.SetMute(muted, std::ptr::null()) }.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn set_capture_volume(level: f32) -> Result<(), String> {
        let devices = capture_devices();
        if devices.is_empty() {
            return Err("no microphone found".into());
        }
        for device in devices {
            unsafe { device.SetMasterVolumeLevelScalar(level, std::ptr::null()) }
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod win {
    use super::Flow;
    pub fn endpoints(_flow: Flow) -> Vec<(String, String)> {
        Vec::new()
    }
    pub fn default_endpoint_id(_flow: Flow) -> Option<String> {
        None
    }
    pub fn set_default(_id: &str) -> Result<(), String> {
        Err("windows only".into())
    }
    pub fn capture_muted() -> Option<bool> {
        None
    }
    pub fn capture_volume() -> Option<f32> {
        None
    }
    pub fn set_capture_muted(_muted: bool) -> Result<(), String> {
        Err("windows only".into())
    }
    pub fn set_capture_volume(_level: f32) -> Result<(), String> {
        Err("windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::unique_names;

    #[test]
    fn repeated_device_names_get_numbers() {
        let devices = unique_names(vec![
            ("a".into(), "Headset".into()),
            ("b".into(), "Speakers".into()),
            ("c".into(), "Headset".into()),
        ]);
        let names: Vec<_> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Headset", "Speakers", "Headset (2)"]);
    }
}
