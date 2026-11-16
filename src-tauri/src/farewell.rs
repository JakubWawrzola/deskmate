//! What the app tells Home Assistant before its Link session ends on purpose
//! (docs/LINK.md, "bye"), and the reconnect after the computer wakes up.
//!
//! - Quitting, signing out, or restarting for an app update while the
//!   Deskmate service is installed: `handover`, so Home Assistant keeps the
//!   computer available while the service takes over.
//! - Windows goes to sleep: `suspend`. The connection stays closed until the
//!   computer wakes up, and then it is opened again at once.
//! - Windows shuts down or restarts: `shutdown`.
//!
//! A `bye` goes only to an integration that announced the `bye` capability.

#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "sleep, wake-up and sign-out are reported by Windows (power_events.rs)"
    )
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use deskmate_service::runner::CAP_BYE;

/// How long the app waits for its session to say goodbye and close.
const FAREWELL_WAIT: Duration = Duration::from_secs(2);
/// A sleep that is never followed by a wake-up event must not keep the app
/// offline for good.
const SUSPEND_FALLBACK: Duration = Duration::from_secs(60);

/// Why the app's connection is about to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The user chose Quit.
    Quit,
    /// The user signs out of Windows.
    SignOut,
    /// The updater starts the installer of a new version, which ends the
    /// app and starts it again.
    Update,
    /// Windows goes to sleep or hibernates.
    Suspend,
    /// Windows shuts down or restarts.
    Shutdown,
}

/// The `reason` of the `bye` to send, or None for no `bye` at all.
pub fn bye_reason(
    ending: Ending,
    service_installed: bool,
    hub_has_bye: bool,
) -> Option<&'static str> {
    if !hub_has_bye {
        return None;
    }
    match ending {
        Ending::Quit | Ending::SignOut | Ending::Update => service_installed.then_some("handover"),
        Ending::Suspend => Some("suspend"),
        Ending::Shutdown => Some("shutdown"),
    }
}

/// A request to the running Link session: send `bye` with this reason as
/// the last frame, close, then answer on `done`.
pub struct Farewell {
    pub reason: &'static str,
    pub done: std::sync::mpsc::Sender<()>,
}

/// Tells Home Assistant why the session ends, when there is a reason to and
/// the integration understands it, and waits until the session has closed.
/// Returns whether a `bye` went out. Blocking: call it off the runtime.
pub fn say_goodbye(app: &AppHandle, ending: Ending) -> bool {
    let state = app.state::<AppState>();
    let hub_has_bye = state.link_info().is_some_and(|info| info.has_cap(CAP_BYE));
    if !hub_has_bye {
        return false;
    }
    // Asked only when it matters: the service manager is a system call.
    let service_installed = matches!(ending, Ending::Quit | Ending::SignOut | Ending::Update)
        && crate::service::is_installed();
    let Some(reason) = bye_reason(ending, service_installed, hub_has_bye) else {
        return false;
    };
    let Some(session) = state.link_farewell() else {
        return false;
    };
    let (done, closed) = std::sync::mpsc::channel();
    if session.send(Farewell { reason, done }).is_err() {
        return false;
    }
    closed.recv_timeout(FAREWELL_WAIT).is_ok()
}

/// Tray > Quit: say goodbye if there is a reason to, then exit.
pub fn quit(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let farewell_app = app.clone();
        let _ =
            tauri::async_runtime::spawn_blocking(move || say_goodbye(&farewell_app, Ending::Quit))
                .await;
        app.exit(0);
    });
}

/// Just before the updater starts the installer, which ends the app: say
/// `handover` if there is a reason to, and keep the connection closed so
/// the installer does not end a fresh session without a goodbye. Returns
/// whether a `bye` went out; then `update_failed` must follow when the
/// installer could not be started.
pub async fn before_update(app: &AppHandle) -> bool {
    let farewell_app = app.clone();
    let said =
        tauri::async_runtime::spawn_blocking(move || say_goodbye(&farewell_app, Ending::Update))
            .await
            .unwrap_or(false);
    if said {
        crate::transport::stop(app).await;
    }
    said
}

/// The installer did not start after `before_update` said goodbye: connect
/// again.
pub fn update_failed(app: &AppHandle) {
    tauri::async_runtime::spawn(crate::transport::restart(app.clone()));
}

/// Suspends seen, and the last suspend a wake-up has answered.
static SUSPENDS: AtomicU64 = AtomicU64::new(0);
static RESUMED: AtomicU64 = AtomicU64::new(0);

const ASLEEP_STATUS: &str =
    "The computer is going to sleep. Deskmate connects again when it wakes up";

/// Windows is about to sleep. Blocking: called from the power event thread,
/// which Windows waits for.
pub fn on_suspend(app: &AppHandle) {
    if !say_goodbye(app, Ending::Suspend) {
        return;
    }
    // The session has closed; keep the connection loop from opening a new
    // one while the network goes away.
    tauri::async_runtime::block_on(crate::transport::stop(app));
    crate::mqtt::set_status(app, false, ASLEEP_STATUS);
    let suspend = SUSPENDS.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SUSPEND_FALLBACK);
        if RESUMED.load(Ordering::SeqCst) < suspend {
            tauri::async_runtime::spawn(crate::transport::restart(app));
        }
    });
}

/// Windows has woken up: open the Link connection again now instead of
/// waiting for the old socket to time out.
pub fn on_resume(app: &AppHandle) {
    RESUMED.store(SUSPENDS.load(Ordering::SeqCst), Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let link = app.state::<AppState>().config.lock().await.transport == "link";
        if link {
            crate::transport::restart(app).await;
        }
    });
}

/// The user signs out, or Windows shuts down or restarts. Blocking: the
/// process may end as soon as this returns.
pub fn on_session_end(app: &AppHandle, signing_out: bool) {
    say_goodbye(
        app,
        if signing_out {
            Ending::SignOut
        } else {
            Ending::Shutdown
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ENDINGS: [Ending; 5] = [
        Ending::Quit,
        Ending::SignOut,
        Ending::Update,
        Ending::Suspend,
        Ending::Shutdown,
    ];

    #[test]
    fn handover_only_with_the_service_installed() {
        for ending in [Ending::Quit, Ending::SignOut, Ending::Update] {
            assert_eq!(bye_reason(ending, true, true), Some("handover"));
            assert_eq!(bye_reason(ending, false, true), None);
        }
    }

    #[test]
    fn suspend_and_shutdown_regardless_of_the_service() {
        for service in [true, false] {
            assert_eq!(bye_reason(Ending::Suspend, service, true), Some("suspend"));
            assert_eq!(
                bye_reason(Ending::Shutdown, service, true),
                Some("shutdown")
            );
        }
    }

    #[test]
    fn never_to_an_integration_without_bye() {
        for ending in ALL_ENDINGS {
            for service in [true, false] {
                assert_eq!(bye_reason(ending, service, false), None, "{ending:?}");
            }
        }
    }

    #[test]
    fn every_reason_is_one_home_assistant_knows() {
        for ending in ALL_ENDINGS {
            if let Some(reason) = bye_reason(ending, true, true) {
                assert!(matches!(reason, "handover" | "suspend" | "shutdown"));
                let frame = deskmate_service::runner::bye_frame(reason);
                assert_eq!(frame["t"], "bye");
                assert_eq!(frame["reason"], reason);
            }
        }
    }
}
