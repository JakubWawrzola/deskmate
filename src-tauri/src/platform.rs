//! What this computer's desktop offers the app. The window reads it once
//! (`platform_capabilities`) and hides or explains what is missing instead
//! of showing controls that would do nothing. On Windows everything is
//! there; the Linux preview looks it up (linux/capabilities.rs).

use serde::Serialize;

/// Mirrored by `PlatformCapabilities` in `src/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    /// `windows` or `linux`.
    pub platform: &'static str,
    /// `windows`, or on Linux `x11`, `wayland` or `unknown`.
    pub session_type: &'static str,
    /// A tray icon can be shown, so the window may hide to it.
    pub tray: bool,
    /// Keyboard shortcuts that work while another program has the focus.
    pub global_shortcuts: bool,
    /// Reading and writing the clipboard.
    pub clipboard: bool,
    /// The title of the window in the foreground.
    pub active_window: bool,
    /// Notifications on the desktop.
    pub notifications: bool,
    /// Buttons on those notifications.
    pub notification_buttons: bool,
    /// Title, artist and controls of the media player that plays.
    pub media: bool,
    /// Volume and mute of the default playback device.
    pub volume: bool,
    /// Locking the session and reading whether it is locked.
    pub lock: bool,
    /// How long the session has been idle.
    pub idle: bool,
    /// Opening a web address in the default browser.
    pub open_url: bool,
    /// Why secrets cannot be kept in the user's keyring, or None when it
    /// is there.
    pub keyring_problem: Option<String>,
}

/// The `info` frame names the platform of the client (docs/LINK.md).
#[cfg(windows)]
pub const CLIENT_PLATFORM: deskmate_core::link::ClientPlatform =
    deskmate_core::link::ClientPlatform::Windows;
#[cfg(target_os = "linux")]
pub const CLIENT_PLATFORM: deskmate_core::link::ClientPlatform =
    deskmate_core::link::ClientPlatform::Linux;

/// Windows has all of it, as it always had.
#[cfg(windows)]
pub fn capabilities() -> Capabilities {
    Capabilities {
        platform: "windows",
        session_type: "windows",
        tray: true,
        global_shortcuts: true,
        clipboard: true,
        active_window: true,
        notifications: true,
        notification_buttons: true,
        media: true,
        volume: true,
        lock: true,
        idle: true,
        open_url: true,
        keyring_problem: None,
    }
}

/// What this Linux desktop offers, looked up once at start.
#[cfg(target_os = "linux")]
pub fn capabilities() -> Capabilities {
    crate::linux::capabilities::current()
}

/// The tray icon could not be created after all: the window must not hide.
#[cfg(target_os = "linux")]
pub fn tray_failed() {
    crate::linux::capabilities::tray_failed();
}

#[cfg(windows)]
fn with_keyring() -> Capabilities {
    capabilities()
}

/// The keyring is asked again on every call: it can be started after the app.
#[cfg(target_os = "linux")]
fn with_keyring() -> Capabilities {
    Capabilities {
        keyring_problem: crate::linux::capabilities::keyring_problem(),
        ..capabilities()
    }
}

#[tauri::command]
pub async fn platform_capabilities() -> Result<Capabilities, String> {
    tauri::async_runtime::spawn_blocking(with_keyring)
        .await
        .map_err(|e| e.to_string())
}
