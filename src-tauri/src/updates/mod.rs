//! Updates: a daily check for a newer Deskmate release (check.rs) and, when
//! the user allows it, its download and install (install.rs).
//!
//! Settings > Updates picks one of three modes (`update_mode`):
//! - `notify`: ask GitHub for the latest release and tell the user. Nothing is
//!   downloaded.
//! - `ask` (default): read the release manifest, then offer "Install now" and
//!   "Later" in a notification and on the Status page.
//! - `auto`: as `ask`, but install without asking once the computer has been
//!   idle for five minutes.
//!
//! `update_check` switches all of it off.
//!
//! Home Assistant has no part in this. No Link or MQTT message reaches this
//! module, and the buttons of the update notification use a link of their own
//! that a notification from Home Assistant cannot carry (prompt.rs).

mod check;
mod decision;
mod install;
mod policy;
mod prompt;

pub use install::restarted_hidden;

use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::Update;

use crate::config::AppConfig;
use crate::notify::{NotifyPayload, OwnButton};
use crate::state::AppState;
use decision::{Situation, Step, UpdateMode};
use policy::ReleaseVersion;
use prompt::Choice;

const RELEASE_PAGE: &str = "https://github.com/JakubWawrzola/deskmate/releases/tag/v";
const RUNNING_VERSION: &str = env!("CARGO_PKG_VERSION");
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the loop looks at the settings, the clock and the idle time.
const LOOK_EVERY: Duration = Duration::from_secs(60);
/// The found release, or the progress of its install, changed.
const UPDATE_EVENT: &str = "deskmate://update";
/// The window should show the Status page, where the update banner is.
const SHOW_EVENT: &str = "deskmate://show-update";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdatePhase {
    Available,
    Downloading,
    Installing,
    Failed,
}

/// The newer release, as the tray menu and the window show it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    /// The release page, built locally from the version.
    pub url: String,
    /// A signed installer for this computer is on offer. False: Deskmate can
    /// only link to the release page.
    pub installable: bool,
    pub phase: UpdatePhase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    /// Why the last attempt stopped, in words for the user.
    pub error: Option<String>,
    /// "Later" holds: the banner and the question wait until tomorrow.
    pub postponed: bool,
    /// The `auto` mode installs this release once the computer is idle.
    pub auto_install: bool,
}

impl UpdateInfo {
    fn found(version: ReleaseVersion, installable: bool) -> Self {
        let version = version.plain();
        Self {
            url: format!("{RELEASE_PAGE}{version}"),
            version,
            installable,
            phase: UpdatePhase::Available,
            downloaded_bytes: 0,
            total_bytes: None,
            error: None,
            postponed: false,
            auto_install: false,
        }
    }

    fn in_progress(&self) -> bool {
        matches!(
            self.phase,
            UpdatePhase::Downloading | UpdatePhase::Installing
        )
    }
}

struct Shared {
    info: Option<UpdateInfo>,
    /// The updater plugin's handle for `info`: where the installer is and the
    /// signature it has to carry. None while only the version is known.
    update: Option<Update>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    info: None,
    update: None,
});

fn shared() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// The newer release found by the last check, if any.
pub fn available() -> Option<UpdateInfo> {
    shared().info.clone()
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn emit(app: &AppHandle) {
    let _ = app.emit(UPDATE_EVENT, available());
}

// ------------------------------------------------------------------ the check

async fn mode(app: &AppHandle) -> UpdateMode {
    UpdateMode::from_config(&app.state::<AppState>().config.lock().await.update_mode)
}

/// Looks for a newer release and remembers what it found. Ok(None): this is
/// the latest version.
async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let manifest = match mode(app).await {
        UpdateMode::Notify => None,
        UpdateMode::Ask | UpdateMode::Auto => match check::installable_release(app).await {
            Ok(found) => Some(found),
            // A release published without a manifest, or a manifest that was
            // refused: the user is still told that a release exists.
            Err(error) => {
                log::info!("no installable update: {error}");
                None
            }
        },
    };
    let found = match manifest {
        Some(Some(update)) => {
            let version = ReleaseVersion::parse(&update.version)
                .ok_or_else(|| "unexpected version in the release manifest".to_string())?;
            Some((version, Some(update)))
        }
        Some(None) => None,
        None => tokio::task::spawn_blocking(check::newer_release)
            .await
            .map_err(|e| e.to_string())??
            .map(|version| (version, None)),
    };
    {
        let mut shared = shared();
        // A download that is running keeps the release it started with.
        if !shared.info.as_ref().is_some_and(UpdateInfo::in_progress) {
            shared.info = found
                .as_ref()
                .map(|(version, update)| UpdateInfo::found(*version, update.is_some()));
            shared.update = found.and_then(|(_, update)| update);
        }
    }
    refresh(app).await;
    Ok(available())
}

// ------------------------------------------------- what to do about a release

#[cfg(windows)]
fn desk_activity() -> (u64, bool) {
    (
        crate::sensors::win::idle_seconds(),
        crate::sensors::win::is_presenting(),
    )
}

#[cfg(not(windows))]
fn desk_activity() -> (u64, bool) {
    (0, false)
}

fn situation(cfg: &AppConfig, info: &UpdateInfo) -> Situation {
    let (idle_secs, presenting) = desk_activity();
    Situation {
        mode: UpdateMode::from_config(&cfg.update_mode),
        installable: info.installable,
        announced: cfg.update_notified == info.version,
        postponed: decision::is_postponed(cfg.update_state.later_until, unix_now()),
        in_progress: info.in_progress(),
        failed: info.phase == UpdatePhase::Failed
            || cfg.update_state.failed_version == info.version,
        idle_secs,
        presenting,
    }
}

/// Brings the two flags that depend on the settings and the clock up to date.
/// Returns whether one of them changed.
fn sync_flags(situation: &Situation) -> bool {
    let mut shared = shared();
    let Some(info) = shared.info.as_mut() else {
        return false;
    };
    let auto_install = decision::installs_when_idle(situation);
    let changed = info.postponed != situation.postponed || info.auto_install != auto_install;
    info.postponed = situation.postponed;
    info.auto_install = auto_install;
    changed
}

/// After a check, a click or a changed setting: the tray entry and the window
/// follow.
async fn refresh(app: &AppHandle) {
    let cfg = app.state::<AppState>().config.lock().await.clone();
    if let Some(info) = available() {
        sync_flags(&situation(&cfg, &info));
    }
    let _ = crate::rebuild_tray_menu(app, &cfg);
    emit(app);
}

/// What the app knows about the found release right now. The flags that the
/// window shows are brought up to date on the way.
async fn look(app: &AppHandle) -> Option<(UpdateInfo, Situation)> {
    let info = available()?;
    let cfg = app.state::<AppState>().config.lock().await.clone();
    let situation = situation(&cfg, &info);
    if sync_flags(&situation) {
        emit(app);
    }
    Some((info, situation))
}

/// Acts on the found release as the mode says: tell, ask or install.
async fn act(app: &AppHandle) {
    let Some((info, situation)) = look(app).await else {
        return;
    };
    match decision::next_step(&situation) {
        Step::Nothing => {}
        Step::Tell => announce(app, &info, false).await,
        Step::Ask => announce(app, &info, true).await,
        Step::Install => {
            if let Err(error) = install::start(app, true) {
                log::info!("automatic update not started: {error}");
            }
        }
    }
}

/// One notification per version. With `ask` it carries "Install now" and
/// "Later"; without, it points at the release page.
async fn announce(app: &AppHandle, info: &UpdateInfo, ask: bool) {
    {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().await;
        if cfg.update_notified == info.version {
            return;
        }
        cfg.update_notified = info.version.clone();
        if let Err(error) = crate::config::save(&cfg) {
            log::warn!("cannot save the settings: {error}");
        }
    }
    let message = if ask {
        "Install it now, or later from the Status page. Settings and pairing are kept."
    } else {
        "Download it from the tray menu or the Status page. Settings and pairing are kept."
    };
    let toast = NotifyPayload {
        title: format!("Deskmate {} is available", info.version),
        message: message.into(),
        image: None,
        actions: Vec::new(),
    };
    let buttons = if ask {
        vec![
            OwnButton {
                title: "Install now".into(),
                link: prompt::issue_link(Choice::Install),
            },
            OwnButton {
                title: "Later".into(),
                link: prompt::issue_link(Choice::Later),
            },
        ]
    } else {
        Vec::new()
    };
    let shown =
        tokio::task::spawn_blocking(move || crate::notify::show_toast_with(&toast, &buttons)).await;
    if let Ok(Err(error)) = shown {
        log::warn!("update notification: {error}");
    }
}

/// "Later": no question and no banner until tomorrow, then both come back.
async fn postpone(app: &AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().await;
        cfg.update_state.later_until = decision::later_deadline(unix_now());
        // Forgotten on purpose, so the question is asked again when the day
        // is over.
        cfg.update_notified.clear();
        crate::config::save(&cfg)?;
    }
    refresh(app).await;
    Ok(())
}

// ------------------------------------------------- state of a running install

/// Takes the found release for installing. Fails when Deskmate has no
/// installer for it, or when the install is already on its way.
fn claim_install() -> Result<Update, String> {
    let mut shared = shared();
    let Shared {
        info: Some(info),
        update: Some(update),
    } = &mut *shared
    else {
        return Err("there is no update that Deskmate can install".into());
    };
    if info.in_progress() {
        return Err("the update is already being installed".into());
    }
    info.phase = UpdatePhase::Downloading;
    info.downloaded_bytes = 0;
    info.total_bytes = None;
    info.error = None;
    Ok(update.clone())
}

/// Keeps the version that was found and drops the installer that came with
/// it. An install that is already running goes on.
fn forget_installer() {
    let mut shared = shared();
    if shared.info.as_ref().is_some_and(UpdateInfo::in_progress) {
        return;
    }
    shared.update = None;
    if let Some(info) = shared.info.as_mut() {
        info.installable = false;
        info.phase = UpdatePhase::Available;
        info.error = None;
    }
}

fn set_progress(app: &AppHandle, downloaded_bytes: u64, total_bytes: Option<u64>) {
    if let Some(info) = shared().info.as_mut() {
        info.downloaded_bytes = downloaded_bytes;
        info.total_bytes = total_bytes;
    }
    emit(app);
}

fn set_installing(app: &AppHandle) {
    if let Some(info) = shared().info.as_mut() {
        info.phase = UpdatePhase::Installing;
    }
    emit(app);
}

fn set_failed(app: &AppHandle, message: &str) {
    if let Some(info) = shared().info.as_mut() {
        info.phase = UpdatePhase::Failed;
        info.error = Some(message.to_string());
        info.auto_install = false;
    }
    emit(app);
}

// ------------------------------------------------------- commands and the tray

/// Handles a click on "Install now" or "Later". Returns whether `arg` was an
/// update link at all, accepted or not, so that the caller does not treat it
/// as an ordinary start of the app.
pub fn handle_link(app: &AppHandle, arg: &str) -> bool {
    if !prompt::is_link(arg) {
        return false;
    }
    match prompt::redeem_link(arg) {
        Some(Choice::Install) => {
            if let Err(error) = install::start(app, false) {
                log::warn!("update not started: {error}");
            }
        }
        Some(Choice::Later) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = postpone(&app).await {
                    log::warn!("cannot postpone the update: {error}");
                }
            });
        }
        None => {
            crate::security::audit("update_prompt", "blocked_unknown_token");
            log::warn!("update link without a valid token ignored");
        }
    }
    true
}

/// "Check now" in Settings. Returns the newer release, if there is one.
#[tauri::command]
pub async fn check_updates_now(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    check(&app).await?;
    act(&app).await;
    Ok(available())
}

/// Opens the release page of the release found by the last check.
pub fn open_release_page() -> Result<(), String> {
    let info = available().ok_or_else(|| "no update available".to_string())?;
    crate::sys_commands::open_url(&info.url)
}

#[tauri::command]
pub fn open_update_page() -> Result<(), String> {
    open_release_page()
}

/// "Install now" on the Status page. Returns once the download has started;
/// progress and the outcome follow as `deskmate://update` events.
#[tauri::command]
pub async fn install_update_now(app: AppHandle) -> Result<(), String> {
    install::start(&app, false)
}

/// "Later" on the Status page.
#[tauri::command]
pub async fn postpone_update(app: AppHandle) -> Result<(), String> {
    postpone(&app).await
}

/// Settings > Updates: auto, ask or notify.
#[tauri::command]
pub async fn set_update_mode(app: AppHandle, mode: String) -> Result<(), String> {
    let mode = UpdateMode::parse(&mode)
        .ok_or_else(|| "update mode must be auto, ask or notify".to_string())?;
    {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().await;
        if cfg.update_mode == mode.as_str() {
            return Ok(());
        }
        cfg.update_mode = mode.as_str().into();
        crate::config::save(&cfg)?;
    }
    // What is known about a release depends on the mode.
    match mode {
        // `notify` installs nothing, so an installer found earlier is dropped.
        UpdateMode::Notify => forget_installer(),
        // `notify` does not read the manifest. Read it now.
        UpdateMode::Ask | UpdateMode::Auto => {
            if available().is_some_and(|info| !info.installable) {
                if let Err(error) = check(&app).await {
                    log::info!("update check failed: {error}");
                }
            }
        }
    }
    refresh(&app).await;
    Ok(())
}

/// The tray entry for a found release.
pub fn on_tray_click(app: &AppHandle) {
    if !available().is_some_and(|info| info.installable) {
        if let Err(error) = open_release_page() {
            log::warn!("cannot open the release page: {error}");
        }
        return;
    }
    // The choices are on the Status page. Asking for them ends "Later", and
    // the user has now seen this version, so no notification follows.
    crate::actions::show_main_window(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        {
            let state = app.state::<AppState>();
            let mut cfg = state.config.lock().await;
            cfg.update_state.later_until = 0;
            if let Some(info) = available() {
                cfg.update_notified = info.version;
            }
            if let Err(error) = crate::config::save(&cfg) {
                log::warn!("cannot save the settings: {error}");
            }
        }
        refresh(&app).await;
        let _ = app.emit(SHOW_EVENT, ());
    });
}

/// Background loop: settle the last install, check a minute after the start
/// and then once a day, and look every minute whether it is time to ask again
/// or, in the `auto` mode, whether the computer is idle.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        install::settle_last_install(&app).await;
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        let mut next_check = Instant::now();
        loop {
            let enabled = app.state::<AppState>().config.lock().await.update_check;
            if enabled {
                if Instant::now() >= next_check {
                    next_check = Instant::now() + CHECK_EVERY;
                    if let Err(error) = check(&app).await {
                        log::info!("update check failed: {error}");
                    }
                }
                act(&app).await;
            } else {
                // Nothing happens by itself, but the banner follows the clock.
                let _ = look(&app).await;
            }
            tokio::time::sleep(LOOK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_found_release_links_to_its_own_page() {
        let version = ReleaseVersion::parse("v0.9.0-rc.1").unwrap();
        let info = UpdateInfo::found(version, true);
        assert_eq!(info.version, "0.9.0");
        assert_eq!(
            info.url,
            "https://github.com/JakubWawrzola/deskmate/releases/tag/v0.9.0"
        );
        assert_eq!(info.phase, UpdatePhase::Available);
        assert!(!info.in_progress());
    }

    #[test]
    fn the_phase_reaches_the_window_in_lower_case() {
        let version = ReleaseVersion::parse("0.9.0").unwrap();
        let mut info = UpdateInfo::found(version, true);
        info.phase = UpdatePhase::Downloading;
        let sent = serde_json::to_value(&info).unwrap();
        assert_eq!(sent["phase"], "downloading");
        assert_eq!(sent["total_bytes"], serde_json::Value::Null);
        assert!(info.in_progress());
    }
}
