//! The "Screen on" sensor. Windows announces every change of the console
//! display's power state, the power setting GUID_CONSOLE_DISPLAY_STATE: off,
//! on or dimmed. PowerSettingRegisterNotification with DEVICE_NOTIFY_CALLBACK
//! delivers it to a callback, so no window is needed, and Windows sends the
//! current state right after the registration.
//!
//! The display state belongs to the whole console, so one value is kept for
//! the app. A new registration replaces it with the current state at once.

use std::sync::atomic::{AtomicU8, Ordering};

/// The values Windows sends for GUID_CONSOLE_DISPLAY_STATE.
const DISPLAY_OFF: u32 = 0;
const DISPLAY_ON: u32 = 1;
const DISPLAY_DIMMED: u32 = 2;

/// Stored before the first notification.
const UNKNOWN: u8 = u8::MAX;

/// The last display state Windows sent, or UNKNOWN.
static DISPLAY_STATE: AtomicU8 = AtomicU8::new(UNKNOWN);

/// Whether a display state counts as on. A dimmed screen is still on.
fn is_on(display_state: u32) -> Option<bool> {
    match display_state {
        DISPLAY_OFF => Some(false),
        DISPLAY_ON | DISPLAY_DIMMED => Some(true),
        _ => None,
    }
}

/// The display state in the data of a power setting notification: a DWORD.
/// None when the data is too short or holds an undocumented value.
fn decode(data: &[u8]) -> Option<u8> {
    let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
    let value = u32::from_ne_bytes(bytes);
    is_on(value)?;
    u8::try_from(value).ok()
}

/// Records a notification's data.
fn remember(data: &[u8]) {
    if let Some(state) = decode(data) {
        DISPLAY_STATE.store(state, Ordering::Relaxed);
    }
}

/// The current state, None before Windows has said.
fn current() -> Option<bool> {
    match DISPLAY_STATE.load(Ordering::Relaxed) {
        UNKNOWN => None,
        state => is_on(u32::from(state)),
    }
}

/// Keeps the registration for the display state while the sensor is on.
/// Dropping it ends the registration.
pub struct Watcher {
    #[cfg(windows)]
    _registration: win::Registration,
}

impl Watcher {
    /// Registers for the display state. None when Windows refuses; the
    /// sensor then publishes nothing.
    pub fn start() -> Option<Self> {
        #[cfg(windows)]
        {
            win::Registration::new().map(|registration| Self {
                _registration: registration,
            })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Whether the screen is on, None until Windows has sent the state.
    pub fn screen_on(&self) -> Option<bool> {
        current()
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use windows::core::GUID;
    use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
    use windows::Win32::System::Power::{
        PowerSettingRegisterNotification, PowerSettingUnregisterNotification,
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY, POWERBROADCAST_SETTING,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_POWERSETTINGCHANGE};

    /// GUID_CONSOLE_DISPLAY_STATE (wdm.h). The windows crate has it behind a
    /// feature that brings in far more than this one value.
    const GUID_CONSOLE_DISPLAY_STATE: GUID =
        GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

    /// The callback Windows calls, in the form the registration takes.
    struct Subscription(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS);

    // SAFETY: the parameters are a function pointer and a null context. They
    // are never written after compilation, and Windows only reads them.
    unsafe impl Sync for Subscription {}

    static SUBSCRIPTION: Subscription = Subscription(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(on_power_setting),
        Context: std::ptr::null_mut(),
    });

    /// Called by Windows on one of its threads for every notification.
    unsafe extern "system" fn on_power_setting(
        _context: *const c_void,
        kind: u32,
        setting: *const c_void,
    ) -> u32 {
        if kind != PBT_POWERSETTINGCHANGE || setting.is_null() {
            return ERROR_SUCCESS.0;
        }
        let header = setting.cast::<POWERBROADCAST_SETTING>();
        // SAFETY: for PBT_POWERSETTINGCHANGE Windows passes a valid
        // POWERBROADCAST_SETTING followed by DataLength bytes of data, alive
        // for the duration of the call. The fields are read without assuming
        // alignment, and no more than DataLength bytes (at most a DWORD) are
        // read from the data.
        let (setting_guid, data) = unsafe {
            let setting_guid = std::ptr::addr_of!((*header).PowerSetting).read_unaligned();
            let length = std::ptr::addr_of!((*header).DataLength).read_unaligned() as usize;
            let data = std::slice::from_raw_parts(
                std::ptr::addr_of!((*header).Data).cast::<u8>(),
                length.min(std::mem::size_of::<u32>()),
            );
            (setting_guid, data)
        };
        if setting_guid == GUID_CONSOLE_DISPLAY_STATE {
            super::remember(data);
        }
        ERROR_SUCCESS.0
    }

    /// A registration for GUID_CONSOLE_DISPLAY_STATE, ended on drop.
    pub struct Registration(HPOWERNOTIFY);

    impl Registration {
        pub fn new() -> Option<Self> {
            let mut handle: *mut c_void = std::ptr::null_mut();
            let recipient = HANDLE(std::ptr::from_ref(&SUBSCRIPTION.0).cast_mut().cast());
            // SAFETY: the GUID and the subscription parameters are statics
            // that outlive the registration, and `handle` is a valid place
            // for the registration handle Windows writes back.
            let result = unsafe {
                PowerSettingRegisterNotification(
                    &GUID_CONSOLE_DISPLAY_STATE,
                    DEVICE_NOTIFY_CALLBACK,
                    recipient,
                    &mut handle,
                )
            };
            if result != ERROR_SUCCESS {
                log::warn!("cannot follow the display state: error {}", result.0);
                return None;
            }
            Some(Self(HPOWERNOTIFY(handle as isize)))
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful registration and is
            // unregistered only here, once.
            let result = unsafe { PowerSettingUnregisterNotification(self.0) };
            if result != ERROR_SUCCESS {
                log::warn!(
                    "cannot end the display state registration: error {}",
                    result.0
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_off_and_on_or_dimmed_is_on() {
        assert_eq!(is_on(DISPLAY_OFF), Some(false));
        assert_eq!(is_on(DISPLAY_ON), Some(true));
        assert_eq!(is_on(DISPLAY_DIMMED), Some(true));
        assert_eq!(is_on(3), None);
        assert_eq!(is_on(u32::MAX), None);
    }

    #[test]
    fn decodes_the_dword_of_a_notification() {
        assert_eq!(decode(&0u32.to_ne_bytes()), Some(0));
        assert_eq!(decode(&1u32.to_ne_bytes()), Some(1));
        assert_eq!(decode(&2u32.to_ne_bytes()), Some(2));
    }

    #[test]
    fn short_or_unknown_data_is_ignored() {
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[1]), None);
        assert_eq!(decode(&[1, 0, 0]), None);
        assert_eq!(decode(&3u32.to_ne_bytes()), None);
        assert_eq!(decode(&256u32.to_ne_bytes()), None);
        assert_eq!(decode(&u32::MAX.to_ne_bytes()), None);
    }

    #[test]
    fn a_notification_updates_the_state_and_garbage_does_not() {
        remember(&0u32.to_ne_bytes());
        assert_eq!(current(), Some(false));
        remember(&[0xff, 0xff]);
        assert_eq!(current(), Some(false));
        remember(&2u32.to_ne_bytes());
        assert_eq!(current(), Some(true));
        remember(&1u32.to_ne_bytes());
        assert_eq!(current(), Some(true));
    }
}
