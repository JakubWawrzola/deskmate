//! Who is using this computer, for a program that runs outside every user's
//! session. The Windows service fills the "Current user" sensor with it, so
//! Home Assistant sees `none` at the sign-in screen instead of the name the
//! app reported last.

/// The value of the sensor while nobody is signed in.
pub const NOBODY: &str = "none";

/// The sensor value for `user`.
pub fn shown(user: Option<String>) -> String {
    user.unwrap_or_else(|| NOBODY.into())
}

/// The account of the session that is in use: at the console or over remote
/// desktop. `None` at the sign-in screen and when every session is
/// disconnected. A locked session still counts, as it does in the app.
#[cfg(windows)]
pub fn active_user() -> Option<String> {
    use windows::Win32::System::RemoteDesktop::{
        WTSActive, WTSEnumerateSessionsW, WTSFreeMemory, WTS_SESSION_INFOW,
    };

    // SAFETY: `sessions` and `count` are valid out-pointers. On success WTS
    // returns an array of `count` entries, which is read as a slice of that
    // length and released with WTSFreeMemory once, after the last read.
    unsafe {
        let mut sessions: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
        let mut count = 0u32;
        if WTSEnumerateSessionsW(None, 0, 1, &mut sessions, &mut count).is_err()
            || sessions.is_null()
        {
            return None;
        }
        let user = std::slice::from_raw_parts(sessions, count as usize)
            .iter()
            .filter(|session| session.State == WTSActive)
            .find_map(|session| user_of(session.SessionId));
        WTSFreeMemory(sessions as *mut _);
        user
    }
}

/// The account name of one session; `None` for a session without a user.
#[cfg(windows)]
fn user_of(session: u32) -> Option<String> {
    use windows::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSUserName,
    };

    // SAFETY: `buffer` and `bytes` are valid out-pointers. On success WTS
    // returns a NUL-terminated UTF-16 string; it is copied before the buffer
    // is released with WTSFreeMemory, as the API requires.
    unsafe {
        let mut buffer = windows::core::PWSTR::null();
        let mut bytes = 0u32;
        if WTSQuerySessionInformationW(None, session, WTSUserName, &mut buffer, &mut bytes).is_err()
            || buffer.is_null()
        {
            return None;
        }
        let name = buffer.to_string().ok();
        WTSFreeMemory(buffer.0 as *mut _);
        name.filter(|name| !name.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nobody_signed_in_is_shown_as_none() {
        assert_eq!(shown(None), "none");
        assert_eq!(shown(Some("anna".into())), "anna");
    }

    /// Who is signed in depends on where the tests run (a build agent may
    /// have no interactive session), so this checks the shape only.
    #[cfg(windows)]
    #[test]
    fn a_reported_user_has_a_name() {
        if let Some(user) = active_user() {
            assert!(!user.is_empty());
        }
    }
}
