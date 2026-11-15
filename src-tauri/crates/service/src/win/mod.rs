//! The Windows layer: where the service's files live, how its parts are put
//! together, and the modules for the service manager, the pipes, access
//! control and installing.

pub mod install;
pub mod pipes;
pub mod security;
pub mod service;

use deskmate_core::link::{ClientPlatform, ClientRole};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;
use windows::core::{GUID, PWSTR};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_ProgramData, FOLDERID_ProgramFilesX64, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
};

use crate::audit::{AuditFile, AuditSink};
use crate::connector::WebSocketConnector;
use crate::power::{PowerExecutor, SystemClock};
use crate::provisioning::{AdminActions, Provisioner};
use crate::runner::{EventReceiver, EventSender, MachineSeat, MachineTelemetry, Parts, Runner};
use crate::secrets::KeyStore;
use pipes::{Listener, PipeNames};

/// File name of the installed service.
pub const EXECUTABLE: &str = "deskmate-service.exe";
/// Folder of the installed service inside Program Files.
const PROGRAM_FOLDER: &str = "Deskmate service";

/// A string the shell allocated, released on drop.
struct ShellString(PWSTR);

impl Drop for ShellString {
    fn drop(&mut self) {
        // SAFETY: the pointer came from SHGetKnownFolderPath, which
        // allocates with CoTaskMemAlloc, and it is released exactly once.
        unsafe { CoTaskMemFree(Some(self.0 .0 as *const c_void)) }
    }
}

/// Asks Windows where a system folder is. Environment variables are not
/// used for this: whoever starts an elevated program can set them.
fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    // SAFETY: `id` is a valid folder id; on success the result is a
    // NUL-terminated string that ShellString releases.
    let raw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }
        .map_err(|e| format!("cannot find a system folder: {e}"))?;
    let raw = ShellString(raw);
    // SAFETY: the string is valid and NUL-terminated until `raw` drops.
    let path = unsafe { raw.0.to_string() }
        .map_err(|_| "a system folder has a name that is not valid text".to_string())?;
    Ok(PathBuf::from(path))
}

/// `%ProgramData%\Deskmate`, the folder above the data folder.
pub fn data_parent() -> Result<PathBuf, String> {
    Ok(known_folder(&FOLDERID_ProgramData)?.join(crate::DATA_FOLDER[0]))
}

/// `%ProgramData%\Deskmate\service`: settings and logs.
pub fn data_dir() -> Result<PathBuf, String> {
    Ok(data_parent()?.join(crate::DATA_FOLDER[1]))
}

/// `%ProgramW6432%\Deskmate service`: the installed executable.
pub fn program_dir() -> Result<PathBuf, String> {
    Ok(known_folder(&FOLDERID_ProgramFilesX64)?.join(PROGRAM_FOLDER))
}

/// What one run of the service works with.
pub struct Launch {
    /// Where the settings and the logs are.
    pub data_dir: PathBuf,
    /// Whether to refuse a data folder that others can write to. Off only
    /// for a test run that brings a folder of its own.
    pub check_folder: bool,
    pub keys: Arc<dyn KeyStore>,
    pub power: Arc<dyn PowerExecutor>,
    pub pipes: PipeNames,
    /// Repeat the log on stderr.
    pub echo_log: bool,
}

/// Why the service could not start. The number is the service-specific exit
/// code the service manager shows.
#[derive(Debug)]
pub enum StartError {
    /// 1: someone other than SYSTEM and Administrators can change the data
    /// folder or the folder above it.
    UnsafeFolder(String),
    /// 2: the log in the data folder cannot be opened.
    Log(String),
    /// 3: a pipe cannot be created; another program may own its name.
    Pipes(String),
    /// 4: the runtime could not start.
    Runtime(String),
}

impl StartError {
    pub fn code(&self) -> u32 {
        match self {
            Self::UnsafeFolder(_) => 1,
            Self::Log(_) => 2,
            Self::Pipes(_) => 3,
            Self::Runtime(_) => 4,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::UnsafeFolder(text)
            | Self::Log(text)
            | Self::Pipes(text)
            | Self::Runtime(text) => text,
        }
    }

    /// What an exit code of the service means, for the person installing it.
    pub fn explain(code: u32) -> &'static str {
        match code {
            1 => "its data folder can be changed by someone other than SYSTEM and Administrators",
            2 => "it cannot write its log",
            3 => "it cannot create its pipes; another program may be using their names",
            4 => "its runtime could not start",
            _ => "it stopped with an error",
        }
    }
}

/// Checks the data folder, opens the log and the pipes and runs the runner
/// until it ends. `ready` is called once the pipes are open, just before the
/// runner starts.
pub fn host(
    launch: Launch,
    events: EventSender,
    incoming: EventReceiver,
    ready: impl FnOnce(),
) -> Result<(), StartError> {
    if launch.check_folder {
        let parent = launch.data_dir.parent().unwrap_or(&launch.data_dir);
        security::check_private_folder(parent).map_err(StartError::UnsafeFolder)?;
        security::check_private_folder(&launch.data_dir).map_err(StartError::UnsafeFolder)?;
    }
    crate::logging::init(&launch.data_dir, launch.echo_log).map_err(StartError::Log)?;
    log::info!("Deskmate service {} starting", env!("CARGO_PKG_VERSION"));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| StartError::Runtime(e.to_string()))?;
    let result = runtime.block_on(async move {
        let admin = Listener::admin(&launch.pipes).map_err(StartError::Pipes)?;
        let status = Listener::status(&launch.pipes).map_err(StartError::Pipes)?;

        let audit: Arc<dyn AuditSink> = Arc::new(AuditFile::new(&launch.data_dir));
        let provisioner = Arc::new(Provisioner::new(&launch.data_dir, launch.keys));
        let (runner, status_rx) = Runner::new(
            Parts {
                connector: WebSocketConnector,
                seat: Arc::new(MachineSeat),
                clock: Arc::new(SystemClock),
                power: launch.power,
                audit: Arc::clone(&audit),
                source: provisioner.clone(),
                telemetry: Arc::new(MachineTelemetry::start),
                role: ClientRole::Service,
                platform: ClientPlatform::Windows,
            },
            incoming,
        );
        let actions = Arc::new(AdminActions::new(provisioner, events, audit));
        let admin_task = tokio::spawn(pipes::serve_admin(admin, actions));
        let status_task = tokio::spawn(pipes::serve_status(status, status_rx));

        ready();
        runner.run().await;
        admin_task.abort();
        status_task.abort();
        Ok(())
    });
    if let Err(error) = &result {
        log::error!("cannot start: {}", StartError::message(error));
    }
    log::info!("Deskmate service stopped");
    log::logger().flush();
    result
}
