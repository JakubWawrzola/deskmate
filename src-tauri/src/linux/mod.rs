//! The Linux desktop (preview). Everything here talks to the user's
//! session: D-Bus for logind, notifications, media players, the tray host
//! and the keyring, and a few programs run with fixed arguments (`wpctl` or
//! `pactl`, `xdg-open`). The sensors and power buttons without a user come
//! from deskmate-core, as for the daemon.
//!
//! What works is listed in `entities` (what Home Assistant gets) and
//! `capabilities` (what the window shows). The rest of the app reaches these
//! modules from the `not(windows)` branches of its own modules.

pub mod bus;
pub mod capabilities;
pub mod entities;
pub mod files;
pub mod media;
pub mod notify;
pub mod open;
pub mod sensors;
pub mod session;
pub mod volume;

#[cfg(test)]
mod seat_tests;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// How long one of the fixed programs may run.
const PROGRAM_TIMEOUT: Duration = Duration::from_secs(3);
/// How often a running program is looked at.
const PROGRAM_POLL: Duration = Duration::from_millis(20);
/// The most output read from a program.
const MAX_OUTPUT_BYTES: u64 = 64 * 1024;

/// Starts what runs in the background on Linux. Called once from setup.
pub fn start(app: &AppHandle) {
    notify::start(app.clone());
    // Looked up now, off the window's first request.
    std::thread::spawn(capabilities::current);
}

/// The program `name` in one of the folders of `PATH`, as a shell finds it.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Runs `program` with `args` and returns what it printed. No shell: every
/// argument reaches the program as it is. The C locale keeps the output in
/// the words the parsers expect. A program that does not end in time is
/// killed.
pub fn run_fixed(program: &Path, args: &[&str]) -> Result<String, String> {
    let name = program
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {name}: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < PROGRAM_TIMEOUT => std::thread::sleep(PROGRAM_POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} did not answer in time"));
            }
            Err(error) => return Err(format!("{name}: {error}")),
        }
    };
    let mut output = String::new();
    if let Some(stdout) = child.stdout.take() {
        stdout
            .take(MAX_OUTPUT_BYTES)
            .read_to_string(&mut output)
            .map_err(|e| format!("{name}: {e}"))?;
    }
    if !status.success() {
        return Err(format!("{name} failed ({status})"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_programs_on_the_path_and_runs_them_without_a_shell() {
        let sh = find_program("sh").expect("every Linux system has sh");
        assert!(sh.is_absolute());
        assert!(find_program("deskmate-no-such-program").is_none());
        // The argument is one argument, never shell code.
        let echoed = run_fixed(&find_program("echo").unwrap(), &["a; echo b"]).unwrap();
        assert_eq!(echoed.trim(), "a; echo b");
        assert!(run_fixed(&find_program("false").unwrap(), &[]).is_err());
    }
}
