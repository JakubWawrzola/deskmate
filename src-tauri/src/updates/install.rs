//! Downloads, verifies and installs the release found by the update check.
//!
//! The order is fixed: the rules in policy.rs, the download with its
//! size limit, the signature check in the updater plugin, and only then the
//! installer. A started installer ends this process, so the attempt is
//! written down first and settled at the next start.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::Update;

use super::policy::{self, ReleaseVersion};
use super::{
    claim_install, decision, emit, set_failed, set_installing, set_progress, unix_now,
    RUNNING_VERSION,
};
use crate::config::{AppConfig, UpdateAttempt};
use crate::notify::NotifyPayload;
use crate::state::AppState;

/// The whole download, from the request to the last byte.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The window hears about progress at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// Why an install attempt stopped before the installer ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Version,
    Address,
    TooLarge,
    Network,
    Signature,
    Format,
    Launch,
}

impl Failure {
    /// The error class written to the audit log.
    fn class(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Address => "address",
            Self::TooLarge => "too_large",
            Self::Network => "network",
            Self::Signature => "signature",
            Self::Format => "format",
            Self::Launch => "launch",
        }
    }

    /// What the Status page shows.
    fn message(self) -> &'static str {
        match self {
            Self::Version => {
                "The offered version is not newer than this one, so nothing was installed."
            }
            Self::Address => {
                "The release points outside Deskmate's GitHub releases, so nothing was downloaded. Use the release page instead."
            }
            Self::TooLarge => {
                "The download was larger than a Deskmate installer and was stopped. Use the release page instead."
            }
            Self::Network => {
                "The download did not finish. Check the internet connection and try again."
            }
            Self::Signature => {
                "The downloaded installer does not carry Deskmate's signature, so it was not run. Try again later or use the release page."
            }
            Self::Format => {
                "The downloaded file is not a Deskmate installer, so it was not run. Use the release page instead."
            }
            Self::Launch => {
                "The installer could not be started. Try again, or download it from the release page."
            }
        }
    }
}

fn classify(error: &tauri_plugin_updater::Error) -> Failure {
    use tauri_plugin_updater::Error;
    match error {
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::SignedVersionMismatch { .. }
        | Error::MissingSignedVersion => Failure::Signature,
        Error::Reqwest(_) | Error::Network(_) => Failure::Network,
        Error::InvalidUpdaterFormat | Error::BinaryNotFoundInArchive => Failure::Format,
        _ => Failure::Launch,
    }
}

/// A version as it goes into the audit log: rebuilt from its numbers.
fn logged(version: &str) -> String {
    ReleaseVersion::parse(version)
        .map(ReleaseVersion::plain)
        .unwrap_or_else(|| "?".into())
}

/// The operation and its result, never what was downloaded.
fn audit(from: &str, to: &str, result: &str) {
    crate::security::audit(
        "update_install",
        &format!("from={} to={} result={result}", logged(from), logged(to)),
    );
}

/// Starts the download and the install of the found release in the
/// background. `auto`: started by the `auto` mode, not by a click.
pub fn start(app: &AppHandle, auto: bool) -> Result<(), String> {
    let update = claim_install()?;
    emit(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let to = update.version.clone();
        if let Err(failure) = run(&app, update, auto).await {
            audit(RUNNING_VERSION, &to, failure.class());
            log::warn!("update to {} stopped: {}", logged(&to), failure.class());
            set_failed(&app, failure.message());
        }
    });
    Ok(())
}

async fn run(app: &AppHandle, mut update: Update, auto: bool) -> Result<(), Failure> {
    // Both were checked when the release was found. They are checked again
    // here because this is the last point before anything is fetched.
    if !policy::is_newer(RUNNING_VERSION, &update.version) {
        return Err(Failure::Version);
    }
    if !policy::download_address_allowed(update.download_url.as_str()) {
        return Err(Failure::Address);
    }
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    // The plugin verifies the signature before it hands the bytes over.
    let installer = download(app, &update).await?;

    let attempt = UpdateAttempt {
        from: RUNNING_VERSION.into(),
        to: logged(&update.version),
        auto,
        hidden: !window_visible(app),
        started_at: unix_now(),
    };
    remember_attempt(app, Some(attempt)).await;
    audit(RUNNING_VERSION, &update.version, "starting_installer");
    set_installing(app);

    // Verifies the bytes once more, writes them to a temporary file and runs
    // it in the installer's update mode. On Windows a started installer ends
    // this process, so this returns only when it could not be started.
    let job = update.clone();
    let outcome = tokio::task::spawn_blocking(move || job.install(installer)).await;
    match outcome {
        Ok(Ok(())) => app.restart(),
        Ok(Err(error)) => {
            remember_attempt(app, None).await;
            Err(classify(&error))
        }
        Err(_) => {
            remember_attempt(app, None).await;
            Err(Failure::Launch)
        }
    }
}

/// Downloads the installer and stops as soon as it grows past the limit.
async fn download(app: &AppHandle, update: &Update) -> Result<Vec<u8>, Failure> {
    let too_large = Arc::new(tokio::sync::Notify::new());
    let stop = too_large.clone();
    let progress_app = app.clone();
    let mut received = 0u64;
    let mut last_progress: Option<Instant> = None;
    let fetch = update.download(
        move |chunk_len, announced| {
            received = received.saturating_add(chunk_len as u64);
            if !policy::download_size_allowed(received, announced) {
                stop.notify_one();
                return;
            }
            if last_progress.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY) {
                last_progress = Some(Instant::now());
                set_progress(&progress_app, received, announced);
            }
        },
        || {},
    );
    let installer = tokio::select! {
        biased;
        () = too_large.notified() => return Err(Failure::TooLarge),
        fetched = fetch => fetched.map_err(|error| classify(&error))?,
    };
    let size = installer.len() as u64;
    if !policy::download_size_allowed(size, None) {
        return Err(Failure::TooLarge);
    }
    set_progress(app, size, Some(size));
    Ok(installer)
}

fn window_visible(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

async fn remember_attempt(app: &AppHandle, attempt: Option<UpdateAttempt>) {
    let state = app.state::<AppState>();
    let mut cfg = state.config.lock().await;
    cfg.update_state.installing = attempt;
    if let Err(error) = crate::config::save(&cfg) {
        log::warn!("cannot save the settings: {error}");
    }
}

/// Whether this start is the installer's restart after an update that began
/// with the window hidden. The app then stays in the tray.
pub fn restarted_hidden(cfg: &AppConfig) -> bool {
    cfg.update_state.installing.as_ref().is_some_and(|attempt| {
        decision::stays_hidden_after_update(
            attempt.hidden,
            &attempt.to,
            RUNNING_VERSION,
            attempt.started_at,
            unix_now(),
        )
    })
}

/// Settles the install that was started before this process: only the next
/// start can tell whether the installer went through.
pub async fn settle_last_install(app: &AppHandle) {
    let (attempt, applied) = {
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().await;
        let Some(attempt) = cfg.update_state.installing.take() else {
            return;
        };
        let applied = policy::same_version(&attempt.to, RUNNING_VERSION);
        if applied {
            cfg.update_state.failed_version.clear();
            cfg.update_state.later_until = 0;
        } else {
            cfg.update_state.failed_version = logged(&attempt.to);
        }
        if let Err(error) = crate::config::save(&cfg) {
            log::warn!("cannot save the settings: {error}");
        }
        (attempt, applied)
    };
    audit(
        &attempt.from,
        &attempt.to,
        if applied { "ok" } else { "not_applied" },
    );
    if !applied {
        log::warn!("the update to {} was not applied", logged(&attempt.to));
        return;
    }
    let toast = NotifyPayload {
        title: format!("Deskmate was updated to {}", logged(&attempt.to)),
        message: "Settings and pairing are unchanged.".into(),
        image: None,
        actions: Vec::new(),
    };
    let shown = tokio::task::spawn_blocking(move || crate::notify::show_toast(&toast)).await;
    if let Ok(Err(error)) = shown {
        log::warn!("update notification: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_updater::Error;

    #[test]
    fn plugin_errors_map_to_audit_classes() {
        assert_eq!(classify(&Error::MissingSignedVersion), Failure::Signature);
        assert_eq!(
            classify(&Error::SignedVersionMismatch {
                signed: "0.8.6".into(),
                announced: "0.9.0".into(),
            }),
            Failure::Signature
        );
        assert_eq!(
            classify(&Error::SignatureUtf8("not base64".into())),
            Failure::Signature
        );
        assert_eq!(
            classify(&Error::Network("status 404".into())),
            Failure::Network
        );
        assert_eq!(classify(&Error::InvalidUpdaterFormat), Failure::Format);
        assert_eq!(
            classify(&Error::Io(std::io::Error::other("no temp folder"))),
            Failure::Launch
        );
    }

    #[test]
    fn every_failure_has_a_class_and_words_for_the_user() {
        let all = [
            Failure::Version,
            Failure::Address,
            Failure::TooLarge,
            Failure::Network,
            Failure::Signature,
            Failure::Format,
            Failure::Launch,
        ];
        let mut classes: Vec<&str> = all.iter().map(|failure| failure.class()).collect();
        for class in &classes {
            assert!(
                class.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{class}"
            );
        }
        classes.sort_unstable();
        classes.dedup();
        assert_eq!(classes.len(), all.len());
        for failure in all {
            assert!(failure.message().ends_with('.'));
        }
    }

    #[test]
    fn versions_are_rebuilt_before_they_reach_the_log() {
        assert_eq!(logged("v0.9.0"), "0.9.0");
        assert_eq!(logged("0.9.0-rc.1"), "0.9.0");
        assert_eq!(logged("0.9.0 result=ok\n1 forged"), "?");
        assert_eq!(logged(""), "?");
    }

    #[test]
    fn only_the_restart_after_a_hidden_update_stays_hidden() {
        let mut cfg = AppConfig::default();
        assert!(!restarted_hidden(&cfg));

        let attempt = UpdateAttempt {
            from: "0.0.1".into(),
            to: RUNNING_VERSION.into(),
            auto: true,
            hidden: true,
            started_at: unix_now(),
        };
        cfg.update_state.installing = Some(attempt.clone());
        assert!(restarted_hidden(&cfg));

        cfg.update_state.installing = Some(UpdateAttempt {
            hidden: false,
            ..attempt.clone()
        });
        assert!(!restarted_hidden(&cfg));

        // The installer did not go through: this is still the old version.
        cfg.update_state.installing = Some(UpdateAttempt {
            to: "999.0.0".into(),
            ..attempt
        });
        assert!(!restarted_hidden(&cfg));
    }
}
