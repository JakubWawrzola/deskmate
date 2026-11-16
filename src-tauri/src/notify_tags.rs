//! Tags and priorities of notifications from Home Assistant (docs/LINK.md,
//! "Notification tags"). A toast with a tag replaces the earlier toast with
//! the same tag, `notify_clear` takes it off the screen and out of the
//! notification centre, and the urgent priority uses the toast scenario that
//! Windows documents for important notifications.

#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "tags and the urgent scenario belong to Windows toasts; the Linux notifications have neither"
    )
)]

use deskmate_core::link::notify::NotifyPriority;

/// Group of every tagged toast from Home Assistant. Windows identifies a
/// toast by tag and group together, so a tag from Home Assistant never
/// matches a toast that Deskmate shows about itself, or one that another
/// program shows under the PowerShell AUMID.
pub const GROUP: &str = "home_assistant";

/// First Windows build with the "urgent" toast scenario (important
/// notifications, which may break through Do Not Disturb).
pub const URGENT_SCENARIO_MIN_BUILD: u32 = 22546;

/// The attribute that the `<toast>` element gets for a priority. Only
/// "urgent" has one, and only where Windows knows the scenario; elsewhere
/// the toast stays an ordinary one with the high toast priority.
pub fn scenario_attribute(priority: NotifyPriority, urgent_supported: bool) -> &'static str {
    if priority == NotifyPriority::Urgent && urgent_supported {
        " scenario=\"urgent\""
    } else {
        ""
    }
}

/// Whether the build number of Windows offers the urgent scenario.
fn urgent_supported_on(build: Option<u32>) -> bool {
    build.is_some_and(|build| build >= URGENT_SCENARIO_MIN_BUILD)
}

#[cfg(windows)]
pub use imp::{clear, remember, urgent_supported};

#[cfg(windows)]
mod imp {
    use std::sync::{Mutex, OnceLock};
    use windows::core::HSTRING;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    /// Tagged toasts shown by this process, oldest first: (tag, toast, AUMID).
    /// Hiding one needs the object; removing it from the notification centre
    /// needs only the tag.
    static SHOWN: Mutex<Vec<(String, ToastNotification, &'static str)>> = Mutex::new(Vec::new());
    /// Enough for the tags an automation uses at the same time.
    const MAX_REMEMBERED: usize = 64;
    /// HRESULT_FROM_WIN32(ERROR_NOT_FOUND): no toast had the tag.
    const NOT_FOUND: i32 = 0x8007_0490_u32 as i32;

    /// Keeps a toast that went on screen with a tag, replacing the one shown
    /// earlier with the same tag (Windows replaces that toast too).
    pub fn remember(tag: &str, toast: ToastNotification, aumid: &'static str) {
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        shown.retain(|(known, _, _)| known != tag);
        if shown.len() >= MAX_REMEMBERED {
            shown.remove(0);
        }
        shown.push((tag.to_string(), toast, aumid));
    }

    /// Takes the toast with this tag off the screen and out of the
    /// notification centre. Succeeds when no toast has the tag. `aumid` is
    /// the identity that toasts are shown under now; a toast remembered under
    /// another one (branding was switched since) is removed there as well.
    pub fn clear(tag: &str, aumid: &'static str) -> Result<(), String> {
        let remembered = {
            let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
            shown
                .iter()
                .position(|(known, _, _)| known == tag)
                .map(|at| shown.remove(at))
        };
        let tag = HSTRING::from(tag);
        let group = HSTRING::from(super::GROUP);
        let mut aumids = vec![aumid];
        if let Some((_, toast, shown_under)) = remembered {
            // A toast that has already left the screen cannot be hidden; the
            // history below is what removes it for good.
            if let Err(error) =
                ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(shown_under))
                    .and_then(|notifier| notifier.Hide(&toast))
            {
                log::debug!("tagged toast no longer on screen ({error})");
            }
            if shown_under != aumid {
                aumids.push(shown_under);
            }
        }
        let history = ToastNotificationManager::History().map_err(|e| format!("toast: {e}"))?;
        for aumid in aumids {
            match history.RemoveGroupedTagWithId(&tag, &group, &HSTRING::from(aumid)) {
                Ok(()) => {}
                Err(error) if error.code().0 == NOT_FOUND => {}
                Err(error) => return Err(format!("toast: {error}")),
            }
        }
        Ok(())
    }

    /// Whether this Windows offers the urgent toast scenario. Read once.
    pub fn urgent_supported() -> bool {
        static SUPPORTED: OnceLock<bool> = OnceLock::new();
        *SUPPORTED.get_or_init(|| super::urgent_supported_on(build_number()))
    }

    fn build_number() -> Option<u32> {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
            .ok()?
            .get_value::<String, _>("CurrentBuildNumber")
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

#[cfg(not(windows))]
pub fn clear(_tag: &str, _aumid: &'static str) -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_urgent_gets_the_scenario_and_only_where_windows_has_it() {
        assert_eq!(
            scenario_attribute(NotifyPriority::Urgent, true),
            " scenario=\"urgent\""
        );
        assert_eq!(scenario_attribute(NotifyPriority::Urgent, false), "");
        assert_eq!(scenario_attribute(NotifyPriority::High, true), "");
        assert_eq!(scenario_attribute(NotifyPriority::Normal, true), "");
    }

    #[test]
    fn the_urgent_scenario_needs_build_22546() {
        assert!(!urgent_supported_on(None));
        assert!(!urgent_supported_on(Some(19045)));
        assert!(!urgent_supported_on(Some(22545)));
        assert!(urgent_supported_on(Some(22546)));
        assert!(urgent_supported_on(Some(26100)));
    }
}
