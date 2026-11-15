//! The "Wake screen" button: a mouse move of zero distance, sent with
//! SendInput. Windows handles it like someone nudging the mouse: it resets the
//! idle timers and turns the displays back on. The pointer stays where it is.
//!
//! The documented alternatives do less. SetThreadExecutionState with
//! ES_DISPLAY_REQUIRED resets the display idle timer, which keeps a screen on
//! but is not meant to turn one back on, and the "on" value of
//! SC_MONITORPOWER is not dependable on current Windows versions (the
//! "Monitors off" button uses its "off" value). Input is what Windows itself
//! waits for to wake the screen.
//!
//! Windows refuses injected input while the session is locked or a program
//! running as administrator is in front; the button then reports an error.
//!
//! Because Windows counts the nudge as the user's input, it postpones the
//! screen saver, the inactivity lock and the automatic update install, and
//! resets the idle time sensor. The button is therefore one of the *Allow
//! input* commands, off until the user turns that on.

#[cfg(windows)]
pub fn wake() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    let inputs = [win::nudge()];
    let size = i32::try_from(std::mem::size_of::<INPUT>())
        .map_err(|_| "wake screen: input structure too large".to_string())?;
    // SAFETY: `inputs` holds one fully initialized INPUT and `size` is the
    // size of that structure, as SendInput requires.
    let sent = unsafe { SendInput(&inputs, size) };
    if sent == 0 {
        return Err(
            "Windows did not accept the input that wakes the screen (locked, or an administrator window is in front)"
                .into(),
        );
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn wake() -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(windows)]
mod win {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };

    /// A relative mouse move of zero distance.
    pub fn nudge() -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::win::nudge;
    use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT_MOUSE, MOUSEEVENTF_MOVE};

    #[test]
    fn the_nudge_is_a_relative_move_of_zero_distance() {
        let input = nudge();
        assert_eq!(input.r#type, INPUT_MOUSE);
        // SAFETY: `nudge` builds the input with its `mi` member.
        let mouse = unsafe { input.Anonymous.mi };
        assert_eq!((mouse.dx, mouse.dy), (0, 0));
        assert_eq!(mouse.dwFlags, MOUSEEVENTF_MOVE);
        assert_eq!(mouse.mouseData, 0);
    }
}
