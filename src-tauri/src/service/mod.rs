//! The Deskmate service from the app's side: whether it runs and with which
//! settings, and the three elevated steps from Settings (install or apply,
//! update, remove). The service itself is `deskmate-service`; ARCHITECTURE.md
//! describes it and SECURITY.md what crosses which pipe.
//!
//! The app never holds administrator rights. It starts the copy of
//! `deskmate-service.exe` that its installer put next to it, through one UAC
//! prompt per step, and hands the settings and the keys over a one-time pipe
//! (`win::pipe`). Keys never go through arguments, files or the
//! environment. Every path and argument is fixed here: nothing received
//! from the network chooses one.

mod provision;
mod view;
#[cfg(windows)]
mod win;

use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

use crate::config::AppConfig;
use crate::state::AppState;
use deskmate_service::runner::CAP_SCOPED_DECLARE;
pub use view::ServiceView;
use view::{Conditions, Observed, Registration};

/// Folder of the bundled service next to `deskmate.exe` (tauri.conf.json,
/// `bundle.resources`).
const BUNDLED_FOLDER: &str = "service";
/// The bundled service is built from the same tree as the app.
const BUNDLED_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the user asked for in the panel.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SetupAction {
    /// Install the service and give it this computer's settings.
    Install,
    /// Replace the running service with the bundled copy; keep its settings.
    Update,
    /// Give the installed service this computer's settings again.
    Apply,
}

/// The bundled `deskmate-service.exe`, if this build has one. Found next to
/// the running program, never through a path from elsewhere.
fn bundled_program() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let program = exe
        .parent()?
        .join(BUNDLED_FOLDER)
        .join(deskmate_service::win::EXECUTABLE);
    program.is_file().then_some(program)
}

/// One elevated step at a time.
static BUSY: AtomicBool = AtomicBool::new(false);

struct Busy;

impl Busy {
    fn claim() -> Result<Self, String> {
        if BUSY.swap(true, Ordering::SeqCst) {
            Err("Another step for the service is still running.".into())
        } else {
            Ok(Self)
        }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// The settings and keys for the service, or why there are none.
fn prepare(
    cfg: &AppConfig,
) -> Result<
    (
        deskmate_service::config::ServiceConfig,
        provision::ServiceKeys,
    ),
    String,
> {
    let keys = provision::load_keys(cfg)?;
    let config = provision::service_config(cfg)?;
    Ok((config, keys))
}

/// Looks at the service and at the app's own settings. Blocking: it asks the
/// service manager, the status pipe and Credential Manager.
fn observe(cfg: &AppConfig, scoped_declare: Option<bool>) -> ServiceView {
    let link = cfg.transport == "link";
    let paired = link
        && crate::config::get_link_key(&cfg.node_id)
            .map(zeroize::Zeroizing::new)
            .is_some();
    let prepared = (link && paired).then(|| prepare(cfg));
    let expected = match &prepared {
        Some(Ok((config, keys))) => provision::fingerprint(config, keys).ok(),
        _ => None,
    };
    let conditions = Conditions {
        bundled: bundled_program().is_some(),
        link,
        paired,
        settings_problem: match &prepared {
            Some(Err(problem)) => Some(problem.clone()),
            _ => None,
        },
        scoped_declare,
        paused: cfg.connection_paused,
    };
    let (registration, line) = read_service();
    view::build_view(&Observed {
        registration,
        line,
        expected_fingerprint: expected.as_deref(),
        bundled_version: BUNDLED_VERSION,
        conditions: &conditions,
    })
}

#[cfg(windows)]
fn read_service() -> (Registration, Option<Result<view::StatusLine, String>>) {
    match win::registration() {
        Ok(win::Registered::No) => (Registration::NotRegistered, None),
        Ok(win::Registered::Yes { process_id: None }) => (Registration::Registered, None),
        Ok(win::Registered::Yes {
            process_id: Some(process_id),
        }) => match win::read_status_line(process_id) {
            Ok(None) => (Registration::Registered, None),
            Ok(Some(line)) => (
                Registration::Registered,
                Some(view::parse_status_line(&line)),
            ),
            Err(problem) => (Registration::Registered, Some(Err(problem))),
        },
        Err(problem) => (Registration::Registered, Some(Err(problem))),
    }
}

#[cfg(not(windows))]
fn read_service() -> (Registration, Option<Result<view::StatusLine, String>>) {
    (Registration::NotRegistered, None)
}

/// Whether the integration of the current Link session can take a service.
fn scoped_declare(app: &AppHandle) -> Option<bool> {
    app.state::<AppState>()
        .link_info()
        .map(|info| info.has_cap(CAP_SCOPED_DECLARE))
}

/// Settings > Deskmate service, and the banner on the Status page.
#[tauri::command]
pub async fn service_status(app: AppHandle) -> Result<ServiceView, String> {
    let cfg = app.state::<AppState>().config.lock().await.clone();
    let scoped = scoped_declare(&app);
    tauri::async_runtime::spawn_blocking(move || observe(&cfg, scoped))
        .await
        .map_err(|e| e.to_string())
}

/// Install, Update or Apply: one UAC prompt. Returns what was done, in words
/// for the user.
#[tauri::command]
pub async fn set_up_service(app: AppHandle, action: SetupAction) -> Result<String, String> {
    let busy = Busy::claim()?;
    let cfg = app.state::<AppState>().config.lock().await.clone();
    let scoped = scoped_declare(&app);
    let done = tauri::async_runtime::spawn_blocking(move || set_up(action, &cfg, scoped))
        .await
        .map_err(|e| e.to_string())??;
    drop(busy);
    Ok(done)
}

/// Remove: one UAC prompt.
#[tauri::command]
pub async fn remove_service() -> Result<String, String> {
    let busy = Busy::claim()?;
    tauri::async_runtime::spawn_blocking(remove)
        .await
        .map_err(|e| e.to_string())??;
    drop(busy);
    Ok("The Deskmate service is removed.".into())
}

#[cfg(windows)]
fn set_up(action: SetupAction, cfg: &AppConfig, scoped: Option<bool>) -> Result<String, String> {
    let current = observe(cfg, scoped);
    let installed = current.phase != view::Phase::NotInstalled;
    let blocked = match action {
        SetupAction::Install if installed => Some("The service is already installed.".into()),
        SetupAction::Install => current.install_blocked,
        _ if !installed => Some("The service is not installed.".into()),
        SetupAction::Apply => current.apply_blocked,
        SetupAction::Update => current.update_blocked,
    };
    if let Some(reason) = blocked {
        return Err(reason);
    }
    let program = bundled_program().ok_or(
        "This copy of Deskmate does not include the service. Install Deskmate from its installer.",
    )?;
    if action == SetupAction::Update {
        win::run_elevated(&program, "install")?;
        return Ok(format!(
            "The service is updated to version {BUNDLED_VERSION}."
        ));
    }
    let (config, keys) = prepare(cfg)?;
    let line = provision::provision_line(&config, &keys)?;
    drop(keys);
    win::install_with_settings(&program, line.as_bytes())?;
    Ok(match action {
        SetupAction::Install => {
            "The Deskmate service is installed and has this computer's settings."
        }
        _ => "The service has this computer's settings.",
    }
    .into())
}

#[cfg(not(windows))]
fn set_up(_action: SetupAction, _cfg: &AppConfig, _scoped: Option<bool>) -> Result<String, String> {
    Err("The Deskmate service exists for Windows only.".into())
}

/// The program that removes the service: the bundled copy, or the installed
/// one when this build has none.
#[cfg(windows)]
fn remover() -> Result<PathBuf, String> {
    match bundled_program() {
        Some(program) => Ok(program),
        None => win::installed_program(),
    }
}

#[cfg(windows)]
fn remove() -> Result<(), String> {
    if !win::is_registered() {
        return Err("The service is not installed.".into());
    }
    win::run_elevated(&remover()?, "uninstall")
}

#[cfg(not(windows))]
fn remove() -> Result<(), String> {
    Err("The Deskmate service exists for Windows only.".into())
}
