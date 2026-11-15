//! The "User state" sensor: Windows' own notification-state check, told apart
//! into five words. The "Presenting or full screen" sensor reads the same
//! check as one on/off value.
//!
//! SHQueryUserNotificationState returns one of seven values. Each maps to one
//! state:
//!
//! | Value | What Windows says | State |
//! |---|---|---|
//! | `QUNS_ACCEPTS_NOTIFICATIONS` (5) | none of the other states, notifications may be shown | `normal` |
//! | `QUNS_BUSY` (2) | a full-screen application is running, or presentation settings are on | `fullscreen` |
//! | `QUNS_APP` (7) | a Windows Store app is running in front | `fullscreen` |
//! | `QUNS_RUNNING_D3D_FULL_SCREEN` (3) | a full-screen, exclusive-mode Direct3D application is running | `game` |
//! | `QUNS_PRESENTATION_MODE` (4) | the user turned on Windows presentation settings | `presentation` |
//! | `QUNS_NOT_PRESENT` (1) | a screen saver is shown, the computer is locked, or another user's session is active | `busy` |
//! | `QUNS_QUIET_TIME` (6) | the quiet hour after a new user's first sign-in | `busy` |
//!
//! Any other value, or a failed call, publishes nothing: Home Assistant keeps
//! the last state until the next reading.

/// The values of QUERY_USER_NOTIFICATION_STATE (shellapi.h).
const QUNS_NOT_PRESENT: i32 = 1;
const QUNS_BUSY: i32 = 2;
const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;
const QUNS_PRESENTATION_MODE: i32 = 4;
const QUNS_ACCEPTS_NOTIFICATIONS: i32 = 5;
const QUNS_QUIET_TIME: i32 = 6;
const QUNS_APP: i32 = 7;

/// The state for a value of SHQueryUserNotificationState, as in the table
/// above. None for a value Windows does not document.
pub fn state_name(value: i32) -> Option<&'static str> {
    match value {
        QUNS_ACCEPTS_NOTIFICATIONS => Some("normal"),
        QUNS_BUSY | QUNS_APP => Some("fullscreen"),
        QUNS_RUNNING_D3D_FULL_SCREEN => Some("game"),
        QUNS_PRESENTATION_MODE => Some("presentation"),
        QUNS_NOT_PRESENT | QUNS_QUIET_TIME => Some("busy"),
        _ => None,
    }
}

/// The current state, or None when Windows cannot tell.
#[cfg(windows)]
pub fn current() -> Option<&'static str> {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    // SAFETY: the call takes no arguments and returns the state by value; it
    // only reads the shell's state for the calling user.
    let state = unsafe { SHQueryUserNotificationState() }.ok()?;
    state_name(state.0)
}

#[cfg(not(windows))]
pub fn current() -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_documented_value_maps_to_one_state() {
        assert_eq!(state_name(QUNS_ACCEPTS_NOTIFICATIONS), Some("normal"));
        assert_eq!(state_name(QUNS_BUSY), Some("fullscreen"));
        assert_eq!(state_name(QUNS_APP), Some("fullscreen"));
        assert_eq!(state_name(QUNS_RUNNING_D3D_FULL_SCREEN), Some("game"));
        assert_eq!(state_name(QUNS_PRESENTATION_MODE), Some("presentation"));
        assert_eq!(state_name(QUNS_NOT_PRESENT), Some("busy"));
        assert_eq!(state_name(QUNS_QUIET_TIME), Some("busy"));
    }

    #[test]
    fn undocumented_values_publish_nothing() {
        for value in [i32::MIN, -1, 0, 8, 100, i32::MAX] {
            assert_eq!(state_name(value), None, "{value}");
        }
    }

    #[test]
    fn the_states_are_the_five_words() {
        let states: std::collections::BTreeSet<&str> = (1..=7).filter_map(state_name).collect();
        assert_eq!(
            states.into_iter().collect::<Vec<_>>(),
            ["busy", "fullscreen", "game", "normal", "presentation"]
        );
    }

    /// The local values are the ones the Windows headers define.
    #[cfg(windows)]
    #[test]
    fn values_match_the_windows_definitions() {
        use windows::Win32::UI::Shell::{
            QUNS_ACCEPTS_NOTIFICATIONS as ACCEPTS, QUNS_APP as APP, QUNS_BUSY as BUSY,
            QUNS_NOT_PRESENT as NOT_PRESENT, QUNS_PRESENTATION_MODE as PRESENTATION,
            QUNS_QUIET_TIME as QUIET, QUNS_RUNNING_D3D_FULL_SCREEN as D3D,
        };
        assert_eq!(NOT_PRESENT.0, QUNS_NOT_PRESENT);
        assert_eq!(BUSY.0, QUNS_BUSY);
        assert_eq!(D3D.0, QUNS_RUNNING_D3D_FULL_SCREEN);
        assert_eq!(PRESENTATION.0, QUNS_PRESENTATION_MODE);
        assert_eq!(ACCEPTS.0, QUNS_ACCEPTS_NOTIFICATIONS);
        assert_eq!(QUIET.0, QUNS_QUIET_TIME);
        assert_eq!(APP.0, QUNS_APP);
    }
}
