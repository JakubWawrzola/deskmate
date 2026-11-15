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

/// The same question for a client without a signed-in user: the Windows
/// service. It holds the connection only while no Deskmate app wants it. It
/// takes a free seat, never announces that it is waiting, and gets false as
/// soon as an app waits for the seat.
///
/// Unlike `turn`, a false while holding the seat does not release it. The
/// caller closes its connection first and then calls `release`, so the app
/// connects only after the service has left.
///
/// What the seat object allows when the service creates it is described at
/// `MACHINE_SEAT_SDDL`.
#[cfg(windows)]
pub fn machine_turn(node: &str) -> bool {
    imp::machine_turn(node)
}
#[cfg(not(windows))]
pub fn machine_turn(_node: &str) -> bool {
    true
}

/// Gives up the seat for `node` if this process holds it, and stops
/// announcing that it waits for it.
pub fn release(_node: &str) {
    #[cfg(windows)]
    imp::release(_node);
}

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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, HANDLE,
        HLOCAL,
    };
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::System::RemoteDesktop::{
        WTSActive, WTSConnectState, WTSFreeMemory, WTSQuerySessionInformationW,
        WTS_CONNECTSTATE_CLASS, WTS_CURRENT_SESSION,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE,
    };

    /// Access to the seat mutex when the service creates it. An app creates
    /// the object with the default descriptor of its account, which only that
    /// account and SYSTEM can open; the service runs as SYSTEM, so its default
    /// would shut every user out. This one allows:
    ///
    /// - SYSTEM (`SY`): everything (`GA`).
    /// - Authenticated Users (`AU`): `SYNCHRONIZE | MUTEX_MODIFY_STATE`
    ///   (0x00100001), so an app of any signed-in account can open the
    ///   object, wait on it and release it. Nothing else: no reading or
    ///   changing of the descriptor, no change of owner, no delete.
    ///
    /// The owner is the one the creating account's token names, and the
    /// descriptor holds no audit entries.
    ///
    /// An app up to 0.8.x asks for full access with `CreateMutexW` and is
    /// refused, which it already reads as "another account holds the seat";
    /// it then announces that it waits, exactly as it does when another
    /// user's Deskmate holds it.
    const MACHINE_SEAT_SDDL: &str = "D:(A;;GA;;;SY)(A;;0x00100001;;;AU)";

    /// A security descriptor built from SDDL text, released on drop.
    struct Descriptor(PSECURITY_DESCRIPTOR);

    impl Descriptor {
        fn from_sddl(sddl: &str) -> windows::core::Result<Self> {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: the text is a valid HSTRING that outlives the call and
            // `descriptor` is a valid out-pointer. On success the system
            // allocates the descriptor, and Drop releases it with LocalFree.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    &HSTRING::from(sddl),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )?;
            }
            Ok(Self(descriptor))
        }

        /// Attributes for creating one object; valid while `self` lives.
        fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0 .0,
                bInheritHandle: false.into(),
            }
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: the pointer came from
            // ConvertStringSecurityDescriptorToSecurityDescriptorW, which
            // allocates with LocalAlloc, and it is released exactly once.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }

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

    /// Says once, not every few seconds, that the seat cannot be checked.
    static MACHINE_SEAT_WARNED: AtomicBool = AtomicBool::new(false);

    pub fn machine_turn(node: &str) -> bool {
        let mut seat = SEAT.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(held) = seat.as_ref() {
            if held.node == node {
                return !someone_waiting(node);
            }
            // Node renamed: the old seat is no longer ours to keep.
            close(held.handle);
            *seat = None;
        }
        if someone_waiting(node) {
            return false;
        }
        let created = Descriptor::from_sddl(MACHINE_SEAT_SDDL).and_then(|descriptor| {
            let attributes = descriptor.attributes();
            // SAFETY: `attributes` points at a descriptor that lives until
            // the end of this closure, and the name is a valid HSTRING that
            // outlives the call.
            unsafe { CreateMutexW(Some(&attributes), false, &seat_name(node)) }
        });
        match created {
            Ok(handle) => {
                // SAFETY: reads this thread's last-error value, which the
                // CreateMutexW call above has just set.
                if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                    // An app holds it.
                    close(handle.0 as isize);
                    return false;
                }
                *seat = Some(Held {
                    node: node.to_string(),
                    handle: handle.0 as isize,
                });
                true
            }
            Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => false,
            Err(error) => {
                // Like the app: a seat that cannot be checked at all does not
                // keep the computer offline.
                if !MACHINE_SEAT_WARNED.swap(true, Ordering::Relaxed) {
                    log::warn!("seat: cannot create the seat mutex ({error}); connecting anyway");
                }
                true
            }
        }
    }

    pub fn release(node: &str) {
        set_waiting(node, false);
        let mut seat = SEAT.lock().unwrap_or_else(|e| e.into_inner());
        if seat.as_ref().is_some_and(|held| held.node == node) {
            if let Some(held) = seat.take() {
                close(held.handle);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows::Win32::System::Threading::{
            OpenMutexW, MUTEX_MODIFY_STATE, SYNCHRONIZATION_ACCESS_RIGHTS,
        };

        fn open_seat(
            node: &str,
            access: SYNCHRONIZATION_ACCESS_RIGHTS,
        ) -> windows::core::Result<()> {
            // SAFETY: the name is a valid HSTRING that outlives the call.
            let handle = unsafe { OpenMutexW(access, false, &seat_name(node)) }?;
            close(handle.0 as isize);
            Ok(())
        }

        /// What an app up to 0.8.x does to take the seat.
        fn create_like_an_app(node: &str) -> windows::core::Result<()> {
            // SAFETY: no security attributes are passed, and the name is a
            // valid HSTRING that outlives the call.
            let handle = unsafe { CreateMutexW(None, false, &seat_name(node)) }?;
            close(handle.0 as isize);
            Ok(())
        }

        /// The only test that touches the seat of this process: the objects
        /// are process-wide, and the node name is unique to this run.
        #[test]
        fn machine_seat_yields_to_a_waiting_app_and_takes_a_free_seat() {
            let node = format!("seat_test_{}", std::process::id());

            assert!(machine_turn(&node), "a free seat is taken");
            assert!(machine_turn(&node), "and kept while nobody waits");

            // An app of any account may open, wait on and release the
            // object, and nothing more.
            open_seat(&node, SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE)
                .expect("the descriptor lets a signed-in user open the seat");
            let refused = create_like_an_app(&node).expect_err("full access is refused");
            assert_eq!(refused.code(), ERROR_ACCESS_DENIED.to_hresult());

            // An app announces that it waits: the service must give way, but
            // the seat stays until the service has closed its connection.
            // SAFETY: no security attributes are passed, and the name is a
            // valid HSTRING that outlives the call.
            let waiting = unsafe { CreateEventW(None, true, false, &waiting_name(&node)) }
                .expect("an app can announce that it waits");
            assert!(!machine_turn(&node));
            open_seat(&node, SYNCHRONIZATION_SYNCHRONIZE).expect("the seat is still held");

            release(&node);
            let gone = open_seat(&node, SYNCHRONIZATION_SYNCHRONIZE).expect_err("released");
            assert_eq!(gone.code(), ERROR_FILE_NOT_FOUND.to_hresult());
            assert!(!machine_turn(&node), "a free seat is left to a waiting app");
            assert!(open_seat(&node, SYNCHRONIZATION_SYNCHRONIZE).is_err());

            // The app is gone again: the service takes the seat back.
            close(waiting.0 as isize);
            assert!(machine_turn(&node));
            release(&node);
        }
    }
}
