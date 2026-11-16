//! What this Linux desktop offers (platform.rs), looked up once when the app
//! starts. The keyring is asked again whenever the window asks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use super::bus;
use crate::platform::Capabilities;

/// The tray host of the AppIndicator / StatusNotifierItem protocol. GNOME
/// has one only with an extension (AppIndicator and KStatusNotifierItem
/// Support); KDE Plasma, Xfce, Cinnamon and MATE have one.
const TRAY_HOST: &str = "org.kde.StatusNotifierWatcher";
/// The Secret Service: GNOME Keyring, KWallet, KeePassXC.
const SECRET_SERVICE: &str = "org.freedesktop.secrets";

static DETECTED: OnceLock<Capabilities> = OnceLock::new();
static TRAY_FAILED: AtomicBool = AtomicBool::new(false);

/// What the desktop offers. The first call looks it up.
pub fn current() -> Capabilities {
    let mut capabilities = DETECTED.get_or_init(detect).clone();
    if TRAY_FAILED.load(Ordering::Relaxed) {
        capabilities.tray = false;
    }
    capabilities
}

/// Called when the tray icon could not be created.
pub fn tray_failed() {
    TRAY_FAILED.store(true, Ordering::Relaxed);
}

/// Why the keyring cannot keep secrets, or None when it can be reached.
pub fn keyring_problem() -> Option<String> {
    let reachable = bus::session()
        .map(|connection| bus::name_available(&connection, SECRET_SERVICE))
        .unwrap_or(false);
    (!reachable).then(|| {
        "No keyring service is running. Deskmate keeps its pairing key in the Secret Service \
         (GNOME Keyring or KWallet): install or start one of them, then save again."
            .to_string()
    })
}

fn detect() -> Capabilities {
    let session_type = session_type(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    );
    let session = bus::session().ok();
    let has_session_bus = session.is_some();
    let (tray, notifications, notification_buttons) = match &session {
        Some(connection) => {
            let notifications = super::notify::service_available(connection);
            let buttons = notifications
                && super::notify::service_capabilities(connection)
                    .iter()
                    .any(|capability| capability == "actions");
            (
                bus::name_has_owner(connection, TRAY_HOST),
                notifications,
                buttons,
            )
        }
        None => (false, false, false),
    };
    let logind = super::session::available();
    let capabilities = Capabilities {
        platform: "linux",
        session_type,
        tray,
        // The global shortcut plugin grabs keys from the X server. Under
        // Wayland that would catch keys only while an X program has the focus.
        global_shortcuts: session_type == "x11",
        // Not in the preview: arboard is built without its Wayland backend.
        clipboard: false,
        active_window: false,
        notifications,
        notification_buttons,
        media: has_session_bus,
        volume: super::volume::available(),
        lock: logind,
        idle: logind,
        open_url: super::open::available(),
        keyring_problem: None,
    };
    log::info!(
        "linux desktop: session {session_type}, tray {tray}, notifications {notifications} \
         (buttons {notification_buttons}), logind {logind}, volume {}, xdg-open {}",
        capabilities.volume,
        capabilities.open_url
    );
    capabilities
}

/// `XDG_SESSION_TYPE` as the session manager set it, or else what the
/// display variables suggest.
fn session_type(declared: Option<&str>, wayland_display: bool, x_display: bool) -> &'static str {
    match declared {
        Some("x11") => "x11",
        Some("wayland") => "wayland",
        _ if wayland_display => "wayland",
        _ if x_display => "x11",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::session_type;

    #[test]
    fn the_declared_session_type_wins_over_the_display_variables() {
        assert_eq!(session_type(Some("wayland"), false, true), "wayland");
        assert_eq!(session_type(Some("x11"), true, true), "x11");
        // XWayland sets DISPLAY in a Wayland session as well.
        assert_eq!(session_type(None, true, true), "wayland");
        assert_eq!(session_type(Some("tty"), false, true), "x11");
        assert_eq!(session_type(None, false, false), "unknown");
    }
}
