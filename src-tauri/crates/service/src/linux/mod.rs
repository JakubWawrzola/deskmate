//! The Linux layer: the daemon that systemd starts as the `deskmate` user,
//! where its files live, how its parts are put together, and the modules for
//! its signals, its status file, its key files, the command line and pairing.
//!
//! The daemon is the computer's only Deskmate client for now, so it declares
//! the machine scope with the role `daemon`. A Linux desktop app will take
//! the connection over through the seat, as the Windows app does with the
//! service.

pub mod app;
pub mod cli;
pub mod events;
pub mod files;
pub mod journal;
pub mod keys;
pub mod pair;
pub mod status_file;

use deskmate_core::config::Dirs;
use deskmate_core::link::{ClientPlatform, ClientRole};
use deskmate_core::secrets::SecretStore;
use std::path::PathBuf;
use std::sync::Arc;

use crate::audit::{AuditFile, AuditSink};
use crate::connector::WebSocketConnector;
use crate::power::{PowerExecutor, SystemClock};
use crate::provisioning::Provisioner;
use crate::runner::{EventReceiver, EventSender, MachineSeat, MachineTelemetry, Parts, Runner};
use keys::KeyFiles;

/// The status file systemd's `RuntimeDirectory=deskmate` holds.
pub const STATUS_FILE: &str = "/run/deskmate/status.json";
/// The account the daemon runs as (`packaging/linux/deskmate.sysusers`).
pub const DAEMON_USER: &str = "deskmate";
/// The systemd unit of the daemon.
pub const UNIT: &str = "deskmate.service";

/// Where the daemon's files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `config.json`. Read-only for the daemon.
    pub config_dir: PathBuf,
    /// The key files and `security.log`.
    pub state_dir: PathBuf,
    /// The status file, one JSON object that any local user may read.
    pub status_file: PathBuf,
}

impl Paths {
    /// `/etc/deskmate`, `/var/lib/deskmate` and `/run/deskmate/status.json`.
    pub fn system() -> Self {
        let dirs = Dirs::system();
        Self {
            config_dir: dirs.config,
            state_dir: dirs.data,
            status_file: PathBuf::from(STATUS_FILE),
        }
    }
}

/// What one run of the daemon works with.
pub struct Launch {
    pub paths: Paths,
    pub power: Arc<dyn PowerExecutor>,
    /// Whether to refuse folders that others can change. Off only for a test
    /// run that brings folders of its own.
    pub check_folders: bool,
    /// Whether a SIGTERM asks systemd if the system is shutting down. Off in
    /// a console run, where nothing but this terminal stops the daemon.
    pub ask_systemd: bool,
}

/// Checks the folders, then runs the runner until it ends. `ready` is called
/// once the signals are set up, just before the runner starts.
pub fn host(
    launch: Launch,
    events: EventSender,
    incoming: EventReceiver,
    ready: impl FnOnce(),
) -> Result<(), String> {
    let paths = launch.paths;
    if launch.check_folders {
        files::check_private_dir(&paths.config_dir)?;
        files::check_private_dir(&paths.state_dir)?;
    }
    deskmate_core::config::set_dirs(Dirs {
        config: paths.config_dir.clone(),
        data: paths.state_dir.clone(),
    })?;
    // Nothing in this process asks a user's keyring.
    deskmate_core::secrets::select(SecretStore::Files(paths.state_dir.clone()))?;
    deskmate_core::seat::linux::set_role(ClientRole::Daemon);
    log::info!("Deskmate daemon {} starting", env!("CARGO_PKG_VERSION"));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("the runtime could not start: {e}"))?;
    let result = runtime.block_on(async move {
        let signals = events::listen(events.clone(), launch.ask_systemd)
            .map_err(|e| format!("cannot listen for signals: {e}"))?;
        let wake = tokio::spawn(events::watch_for_wake(events));

        let audit: Arc<dyn AuditSink> = Arc::new(AuditFile::new(&paths.state_dir));
        // Settings from the config folder, keys from the state folder.
        let keys = Arc::new(KeyFiles::new(paths.state_dir.clone()));
        let source = Arc::new(Provisioner::new(&paths.config_dir, keys));
        let (runner, status_rx) = Runner::new(
            Parts {
                connector: WebSocketConnector,
                seat: Arc::new(MachineSeat),
                clock: Arc::new(SystemClock),
                power: launch.power,
                audit,
                source,
                telemetry: Arc::new(MachineTelemetry::start),
                role: ClientRole::Daemon,
                platform: ClientPlatform::Linux,
            },
            incoming,
        );
        let status_task = tokio::spawn(status_file::serve(paths.status_file.clone(), status_rx));

        ready();
        runner.run().await;
        signals.abort();
        wake.abort();
        // The last status has been written; a file left behind would claim a
        // daemon that no longer runs.
        let _ = status_task.await;
        status_file::remove(&paths.status_file);
        Ok::<(), String>(())
    });
    if let Err(error) = &result {
        log::error!("cannot start: {error}");
    }
    log::info!("Deskmate daemon stopped");
    log::logger().flush();
    result
}
