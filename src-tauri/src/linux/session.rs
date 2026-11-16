//! The user's session as logind sees it: whether it is locked, whether it is
//! idle, and locking it. logind's `auto` session is the session this process
//! runs in, or the user's graphical session when the process runs outside
//! one (started by the systemd user manager, for example).
//!
//! The idle hint is set by the desktop, not by logind: GNOME and KDE set it
//! after their own idle delay. Until then the session counts as active.

use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use std::time::{SystemTime, UNIX_EPOCH};

use super::bus;

const LOGIND: &str = "org.freedesktop.login1";
const SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";

fn session_property<T>(name: &str) -> Result<T, String>
where
    T: for<'b> dbus::arg::Get<'b> + 'static,
{
    let connection = bus::system()?;
    connection
        .with_proxy(LOGIND, SESSION_PATH, bus::CALL_TIMEOUT)
        .get::<T>(SESSION_INTERFACE, name)
        .map_err(|e| format!("logind: {e}"))
}

/// Whether logind knows the session this app runs in.
pub fn available() -> bool {
    session_property::<String>("Id").is_ok_and(|id| !id.is_empty())
}

/// Locks the session: the desktop shows its lock screen.
pub fn lock() -> Result<(), String> {
    let connection = bus::system()?;
    connection
        .with_proxy(LOGIND, SESSION_PATH, bus::CALL_TIMEOUT)
        .method_call::<(), _, _, _>(SESSION_INTERFACE, "Lock", ())
        .map_err(|e| format!("cannot lock the session: {e}"))
}

/// Whether the lock screen is up. None when logind cannot be asked.
pub fn locked() -> Option<bool> {
    session_property::<bool>("LockedHint").ok()
}

/// Seconds since the desktop marked the session idle, 0 while it is not.
pub fn idle_seconds() -> Option<u64> {
    if !session_property::<bool>("IdleHint").ok()? {
        return Some(0);
    }
    let since_usec = session_property::<u64>("IdleSinceHint").ok()?;
    let now_usec = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_micros();
    Some(idle_seconds_between(since_usec, now_usec))
}

/// Whole seconds from `since_usec` to `now_usec`, both microseconds of the
/// real-time clock; 0 when the clock went back.
fn idle_seconds_between(since_usec: u64, now_usec: u128) -> u64 {
    let elapsed = now_usec.saturating_sub(u128::from(since_usec)) / 1_000_000;
    u64::try_from(elapsed).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::idle_seconds_between;

    #[test]
    fn idle_time_counts_whole_seconds_and_never_goes_negative() {
        assert_eq!(idle_seconds_between(1_000_000, 1_000_000), 0);
        assert_eq!(idle_seconds_between(1_000_000, 4_999_999), 3);
        assert_eq!(idle_seconds_between(5_000_000, 1_000_000), 0);
    }
}
