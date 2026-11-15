//! Sleep, wake-up, sign-out and shutdown, as Windows announces them: a
//! hidden top-level window on its own thread receives the broadcasts that
//! Tauri's windows do not pass on (`WM_POWERBROADCAST`, `WM_ENDSESSION`).
//! What happens then is `farewell`.

use std::sync::OnceLock;
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Starts listening. Called once from setup; a second call does nothing.
pub fn spawn(app: AppHandle) {
    if APP.set(app).is_err() {
        return;
    }
    #[cfg(windows)]
    if let Err(error) = std::thread::Builder::new()
        .name("power-events".into())
        .spawn(win::run)
    {
        log::warn!("power events: cannot start the listener: {error}");
    }
}

#[cfg(windows)]
mod win {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        RegisterClassW, ENDSESSION_LOGOFF, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND,
        WINDOW_EX_STYLE, WM_ENDSESSION, WM_POWERBROADCAST, WM_QUERYENDSESSION, WNDCLASSW,
        WS_OVERLAPPED,
    };

    use super::APP;
    use crate::farewell;

    const CLASS: PCWSTR = w!("DeskmatePowerEvents");

    /// The hidden window, destroyed on drop.
    struct Hidden(HWND);

    impl Drop for Hidden {
        fn drop(&mut self) {
            // SAFETY: the window was created by this thread and is destroyed
            // once, from the same thread.
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }

    fn create() -> Result<Hidden, String> {
        // SAFETY: no name: the handle of this program's own module.
        let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised; its strings are static.
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err("cannot register the window class".into());
        }
        // SAFETY: the class was registered above; no parent, no menu, no
        // extra data. A top-level window that is never shown, because
        // message-only windows get no broadcasts.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS,
                w!("Deskmate power events"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        Ok(Hidden(window))
    }

    pub fn run() {
        let window = match create() {
            Ok(window) => window,
            Err(error) => {
                log::warn!("power events: {error}");
                return;
            }
        };
        let mut message = MSG::default();
        // SAFETY: `message` is a valid out-pointer; messages for any window
        // of this thread are fetched and dispatched to their procedures.
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            // SAFETY: `message` was just filled by GetMessageW.
            unsafe {
                DispatchMessageW(&message);
            }
        }
        drop(window);
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let Some(app) = APP.get() else {
            // SAFETY: the arguments are the ones Windows passed in.
            return unsafe { DefWindowProcW(window, message, wparam, lparam) };
        };
        match message {
            WM_POWERBROADCAST => {
                match u32::try_from(wparam.0).unwrap_or_default() {
                    PBT_APMSUSPEND => farewell::on_suspend(app),
                    PBT_APMRESUMEAUTOMATIC => farewell::on_resume(app),
                    _ => {}
                }
                LRESULT(1)
            }
            // Deskmate never holds up a sign-out or a shutdown.
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wparam.0 != 0 {
                    // The low 32 bits carry the ENDSESSION_* flags.
                    let flags = lparam.0 as u32;
                    farewell::on_session_end(app, flags & ENDSESSION_LOGOFF != 0);
                }
                LRESULT(0)
            }
            // SAFETY: the arguments are the ones Windows passed in.
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }
}
