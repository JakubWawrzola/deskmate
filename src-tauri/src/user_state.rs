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
//!
//! Windows reports `game` only for a game that owns the screen (exclusive
//! full screen). Most games today run in a window without a border that
//! covers the screen, and Windows calls that `fullscreen`, the same as a
//! film in a browser. So a `fullscreen` whose program in front is a game
//! becomes `game`: one that was started from the folder of a game store, or
//! one the user listed on the Sensors page.

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

/// Folders that game stores install into, in lower case. A program that
/// runs from one of them is a game.
const GAME_FOLDERS: [&str; 8] = [
    r"\steamapps\common\",
    r"\epic games\",
    r"\gog games\",
    r"\gog galaxy\games\",
    r"\xboxgames\",
    r"\riot games\",
    r"\ea games\",
    r"\ubisoft game launcher\games\",
];
/// Whether the program at `path` is a game: it runs from a game store's
/// folder, or its file name is one of `listed` (lower case, with ".exe").
fn is_game(path: &str, listed: &[String]) -> bool {
    let path = path.to_lowercase();
    let file = path.rsplit('\\').next().unwrap_or_default();
    GAME_FOLDERS.iter().any(|folder| path.contains(folder))
        || listed.iter().any(|program| program == file)
}

/// `state` with `fullscreen` turned into `game` when the program in front,
/// at `foreground`, is a game. Every other state stays what Windows said.
pub fn with_games(
    state: &'static str,
    foreground: Option<&str>,
    listed: &[String],
) -> &'static str {
    match foreground {
        Some(path) if state == "fullscreen" && is_game(path, listed) => "game",
        _ => state,
    }
}

/// The current state, or None when Windows cannot tell. `listed` are the
/// programs the user counts as games.
#[cfg(windows)]
pub fn current(listed: &[String]) -> Option<&'static str> {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    // SAFETY: the call takes no arguments and returns the state by value; it
    // only reads the shell's state for the calling user.
    let state = unsafe { SHQueryUserNotificationState() }.ok()?;
    let state = state_name(state.0)?;
    if state != "fullscreen" {
        return Some(state);
    }
    // The path is looked at here and goes nowhere else.
    let foreground = crate::sensors::win::foreground_program_path();
    Some(with_games(state, foreground.as_deref(), listed))
}

#[cfg(not(windows))]
pub fn current(_listed: &[String]) -> Option<&'static str> {
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
    fn a_borderless_game_from_a_store_folder_counts_as_a_game() {
        for path in [
            r"D:\SteamLibrary\steamapps\common\ELDEN RING\Game\eldenring.exe",
            r"C:\Program Files (x86)\Steam\steamapps\common\Hades\x64\Hades.exe",
            r"C:\Program Files\Epic Games\Fortnite\Binaries\Win64\FortniteClient.exe",
            r"E:\GOG Games\The Witcher 3\bin\x64\witcher3.exe",
            r"C:\XboxGames\Forza Horizon 5\Content\ForzaHorizon5.exe",
            r"C:\Riot Games\VALORANT\live\VALORANT.exe",
        ] {
            assert_eq!(with_games("fullscreen", Some(path), &[]), "game", "{path}");
        }
    }

    #[test]
    fn a_film_or_a_browser_stays_fullscreen() {
        for path in [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            // The store itself is not a game.
            r"C:\Program Files (x86)\Steam\steam.exe",
            r"C:\Program Files\Epic Games Launcher\Portal\EpicGamesLauncher.exe",
        ] {
            assert_eq!(
                with_games("fullscreen", Some(path), &[]),
                "fullscreen",
                "{path}"
            );
        }
        assert_eq!(with_games("fullscreen", None, &[]), "fullscreen");
    }

    #[test]
    fn a_listed_program_counts_as_a_game_wherever_it_is_installed() {
        let listed = vec!["minecraft.exe".to_string()];
        assert_eq!(
            with_games(
                "fullscreen",
                Some(r"C:\Games\Minecraft\Minecraft.exe"),
                &listed
            ),
            "game"
        );
        assert_eq!(
            with_games("fullscreen", Some(r"C:\Games\Other\other.exe"), &listed),
            "fullscreen"
        );
        // A folder with the program's name is not the program.
        assert_eq!(
            with_games("fullscreen", Some(r"C:\minecraft.exe\viewer.exe"), &listed),
            "fullscreen"
        );
    }

    #[test]
    fn only_fullscreen_is_ever_turned_into_game() {
        let steam = r"D:\steamapps\common\Game\game.exe";
        for state in ["normal", "presentation", "busy", "game"] {
            assert_eq!(with_games(state, Some(steam), &[]), state);
        }
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
