//! One connection per paired computer, even with several Windows users.
//!
//! Every Windows account runs its own Deskmate. When two accounts are paired as
//! the same computer (same node id, see "Pair another Windows user" in Home
//! Assistant), both would connect and Home Assistant would keep replacing one
//! session with the other. A named mutex in the Global namespace, visible to
//! every logon session, lets only one of them hold the connection.
//!
//! The account someone is actually using wins: the holder steps back when its
//! own session is disconnected (fast user switching) while an instance in an
//! active session is waiting, announced by a second named object. With nobody
//! waiting the holder keeps going, so a single user's PC never notices any of
//! this, and an RDP session left disconnected keeps reporting.

use std::time::Duration;
use tokio::sync::watch;

/// Called before connecting and every few seconds while connected. Returns
/// whether this process may hold the connection for `node` right now; when it
/// returns false after holding it, the seat has already been released.
#[cfg(windows)]
pub fn turn(node: &str) -> bool {
    imp::turn(node)
}
#[cfg(target_os = "linux")]
pub fn turn(node: &str) -> bool {
    linux::turn(node)
}

/// The same contract on Linux, for the daemon and the desktop apps.
#[cfg(target_os = "linux")]
pub mod linux;

/// How often a connected session checks whether to hand over.
pub const CHECK_EVERY: Duration = Duration::from_secs(5);

/// What a waiting process tells its user.
#[cfg(windows)]
pub const WAITING_STATUS: &str =
    "Another Windows user on this computer holds the connection. Waiting...";
#[cfg(target_os = "linux")]
pub const WAITING_STATUS: &str =
    "Another Deskmate on this computer holds the connection. Waiting...";

/// Waits until this process may connect as `node`. Returns false if `stop`
/// fired first. `announce` is called once, with `WAITING_STATUS`, when the
/// seat turns out to be taken.
pub async fn wait_for_turn(
    node: &str,
    stop: &mut watch::Receiver<bool>,
    mut announce: impl FnMut(&str),
) -> bool {
    let mut announced = false;
    loop {
        if *stop.borrow() {
            stand_down(node);
            return false;
        }
        if turn(node) {
            return true;
        }
        if !announced {
            announce(WAITING_STATUS);
            announced = true;
        }
        tokio::select! {
            _ = stop.changed() => {
                stand_down(node);
                return false;
            }
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
        }
    }
}

/// Stops announcing that this process is waiting for `node`.
fn stand_down(_node: &str) {
    #[cfg(windows)]
    imp::set_waiting(_node, false);
    #[cfg(target_os = "linux")]
    linux::set_waiting(_node, false);
}

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, HANDLE,
    };
    use windows::Win32::System::RemoteDesktop::{
        WTSActive, WTSConnectState, WTSFreeMemory, WTSQuerySessionInformationW,
        WTS_CONNECTSTATE_CLASS, WTS_CURRENT_SESSION,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE,
    };

    /// Raw handles kept as integers: HANDLE wraps a pointer and is not Send.
    struct Held {
        node: String,
        handle: isize,
    }

    /// The seat this process holds, if any.
    static SEAT: Mutex<Option<Held>> = Mutex::new(None);
    /// The "I am waiting" object this process holds, if any.
    static WAITING: Mutex<Option<Held>> = Mutex::new(None);

    fn seat_name(node: &str) -> HSTRING {
        HSTRING::from(format!("Global\\Deskmate.Seat.{node}"))
    }

    fn waiting_name(node: &str) -> HSTRING {
        HSTRING::from(format!("Global\\Deskmate.Seat.{node}.waiting"))
    }

    fn close(handle: isize) {
        // SAFETY: every caller passes a handle this module opened and still
        // owns, and forgets it right after this call.
        unsafe {
            let _ = CloseHandle(HANDLE(handle as *mut _));
        }
    }

    /// Whether the session this process runs in is the one in use (console or
    /// a connected remote desktop). Locked still counts as active; only
    /// switching to another account disconnects a session. Unknown = active.
    fn session_active() -> bool {
        // SAFETY: `buffer` and `bytes` are valid out-pointers. On success WTS
        // returns a buffer holding one WTS_CONNECTSTATE_CLASS; it is read once
        // and released with WTSFreeMemory, as the API requires.
        unsafe {
            let mut buffer = windows::core::PWSTR::null();
            let mut bytes = 0u32;
            if WTSQuerySessionInformationW(
                None,
                WTS_CURRENT_SESSION,
                WTSConnectState,
                &mut buffer,
                &mut bytes,
            )
            .is_err()
                || buffer.is_null()
            {
                return true;
            }
            let state = *(buffer.0 as *const WTS_CONNECTSTATE_CLASS);
            WTSFreeMemory(buffer.0 as *mut _);
            state == WTSActive
        }
    }

    /// Whether some other process announced it is waiting for `node`. An
    /// object created by another Windows account is not accessible to us, and
    /// "access denied" still proves it exists.
    fn someone_waiting(node: &str) -> bool {
        // SAFETY: the name is a valid HSTRING that outlives the call.
        match unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, &waiting_name(node)) } {
            Ok(handle) => {
                close(handle.0 as isize);
                true
            }
            Err(error) => error.code() == ERROR_ACCESS_DENIED.to_hresult(),
        }
    }

    pub fn set_waiting(node: &str, waiting: bool) {
        let mut slot = WAITING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(held) = slot.as_ref() {
            if waiting && held.node == node {
                return;
            }
            close(held.handle);
            *slot = None;
        }
        if waiting {
            // Fails with "access denied" when another account already waits;
            // the announcement exists then anyway.
            // SAFETY: no security attributes are passed, and the name is a
            // valid HSTRING that outlives the call.
            if let Ok(handle) = unsafe { CreateEventW(None, true, false, &waiting_name(node)) } {
                *slot = Some(Held {
                    node: node.to_string(),
                    handle: handle.0 as isize,
                });
            }
        }
    }

    pub fn turn(node: &str) -> bool {
        let active = session_active();
        let mut seat = SEAT.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(held) = seat.as_ref() {
            if held.node == node {
                if !active && someone_waiting(node) {
                    log::info!(
                        "seat: another Windows user is active on this computer, handing over"
                    );
                    close(held.handle);
                    *seat = None;
                    return false;
                }
                return true;
            }
            // Node renamed: the old seat is no longer ours to keep.
            close(held.handle);
            *seat = None;
        }

        // Our own announcement must not count when we check for others.
        set_waiting(node, false);
        if !active && someone_waiting(node) {
            return false;
        }
        // SAFETY: no security attributes are passed, and the name is a valid
        // HSTRING that outlives the call.
        match unsafe { CreateMutexW(None, false, &seat_name(node)) } {
            Ok(handle) => {
                // SAFETY: reads this thread's last-error value, which the
                // CreateMutexW call above has just set.
                if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                    // Another process of this account holds it.
                    close(handle.0 as isize);
                } else {
                    *seat = Some(Held {
                        node: node.to_string(),
                        handle: handle.0 as isize,
                    });
                    return true;
                }
            }
            // ERROR_ACCESS_DENIED: another account holds it.
            Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => {}
            Err(error) => {
                // The seat cannot be checked at all; do not stop the app from
                // connecting because of that.
                log::warn!("seat: cannot create the seat mutex ({error}); connecting anyway");
                return true;
            }
        }
        if active {
            set_waiting(node, true);
        }
        false
    }
}
