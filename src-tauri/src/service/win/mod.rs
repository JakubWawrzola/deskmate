//! The Windows half of setting up the service: the elevated steps, the
//! one-time pipe and what the service manager and the status pipe say.

mod account;
mod elevate;
mod handle;
mod pipe;
mod scm;

use std::path::{Path, PathBuf};
use std::time::Instant;

use elevate::ElevatedProcess;
use pipe::{OneTimePipe, OFFER_LIMIT};
pub use scm::{is_registered, read_status_line, registration, Registered};

/// What an exit code of `deskmate-service install` or `uninstall` means.
fn exit_meaning(code: u32) -> String {
    match code {
        1 => "the step failed".into(),
        2 => "the service did not understand what it was asked to do".into(),
        5 => "it did not get administrator rights".into(),
        other => format!("it ended with code {other}"),
    }
}

/// `deskmate-service install`: installed, but the program offering the
/// settings could not be checked, so they were not taken.
const SERVER_UNCHECKED_EXIT: u32 = 6;
const SERVER_UNCHECKED_MESSAGE: &str = "The service is installed but has no settings: Windows did not let it check that they come from this Deskmate. Approve the prompt with the account that runs Deskmate, then press Apply.";

const STILL_RUNNING: &str =
    "The step with administrator rights is still running after two minutes. Look at this panel again in a moment.";

/// Runs `program arguments` with one UAC prompt and waits for it.
pub fn run_elevated(program: &Path, arguments: &str) -> Result<(), String> {
    let process = ElevatedProcess::start(program, arguments).map_err(|e| e.message())?;
    match process.wait(Instant::now() + OFFER_LIMIT) {
        Some(0) => Ok(()),
        Some(code) => Err(format!(
            "The service step did not work: {}.",
            exit_meaning(code)
        )),
        None => Err(STILL_RUNNING.into()),
    }
}

/// Installs or updates the service with one UAC prompt and hands it `line`
/// through a one-time pipe. Only the installer that was started here can
/// read the pipe; it does so after the service is running.
pub fn install_with_settings(program: &Path, line: &[u8]) -> Result<(), String> {
    let pipe = OneTimePipe::create()?;
    let process = ElevatedProcess::start(
        program,
        &format!("install --provision-pipe {}", pipe.name()),
    )
    .map_err(|e| e.message())?;
    let deadline = Instant::now() + OFFER_LIMIT;
    let offered = pipe.offer(line, process.id(), deadline, Some(process.handle()));
    let code = process.wait(deadline);
    drop(pipe);
    match (code, offered) {
        (Some(0), Ok(())) => Ok(()),
        (None, _) => Err(STILL_RUNNING.into()),
        // The installer could not check that this app offers the settings.
        (Some(SERVER_UNCHECKED_EXIT), _) => Err(SERVER_UNCHECKED_MESSAGE.into()),
        (Some(0), Err(error)) => Err(format!(
            "The service is installed, but {}.",
            error.message()
        )),
        (Some(code), Ok(())) => Err(format!(
            "The service is installed but did not take the settings: {}.",
            exit_meaning(code)
        )),
        (Some(code), Err(_)) => Err(format!(
            "The service could not be installed: {}. Nothing was handed over.",
            exit_meaning(code)
        )),
    }
}

/// The installed copy in Program Files, which only administrators can change.
pub fn installed_program() -> Result<PathBuf, String> {
    Ok(deskmate_service::win::program_dir()?.join(deskmate_service::win::EXECUTABLE))
}
