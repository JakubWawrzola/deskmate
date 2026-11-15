//! The daemon's status: `/run/deskmate/status.json`, one JSON object with the
//! fields of the Windows service's status line (`crate::status`). Any local
//! user may read it, so it holds what that line holds and nothing more. It is
//! written whole whenever the state changes and removed when the daemon
//! stops.

use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use tokio::sync::watch;

use super::files::write_whole;
use crate::status::Status;

/// Readable by everyone, writable by the daemon.
const STATUS_FILE_MODE: u32 = 0o644;
/// A status file larger than this was not written by the daemon.
const MAX_STATUS_BYTES: u64 = 4096;

/// Writes the status now and after every change, until the runner is gone.
pub async fn serve(path: PathBuf, mut status: watch::Receiver<Status>) {
    loop {
        let line = status.borrow_and_update().line();
        let target = path.clone();
        let written = tokio::task::spawn_blocking(move || {
            write_whole(&target, line.as_bytes(), STATUS_FILE_MODE, None)
        })
        .await;
        match written {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::error!("status file: {error}"),
            Err(_) => log::error!("status file: the write did not finish"),
        }
        if status.changed().await.is_err() {
            return;
        }
    }
}

/// Removes the status file. One that is already gone is not an error.
pub fn remove(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => log::error!("status file: cannot remove {}: {error}", path.display()),
    }
}

/// The status file as the daemon wrote it: one JSON object on one line.
pub fn read(path: &Path) -> Result<String, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(format!(
                "{} does not exist: the daemon is not running. See: systemctl status deskmate",
                path.display()
            ))
        }
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let mut text = String::new();
    file.take(MAX_STATUS_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let valid = text.len() as u64 <= MAX_STATUS_BYTES
        && serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|value| value.is_object());
    if !valid {
        return Err(format!("{} is not a status file", path.display()));
    }
    Ok(text.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::files::tests::Scratch;
    use crate::status::State;
    use std::os::unix::fs::PermissionsExt;

    fn status(state: State) -> Status {
        Status {
            state,
            node_id: "office_pc".into(),
            detail: "Connected to Home Assistant.".into(),
            fingerprint: "ab".repeat(32),
        }
    }

    #[tokio::test]
    async fn follows_the_state_and_ends_with_the_runner() {
        let scratch = Scratch::new("status");
        let path = scratch.0.join("status.json");
        let (sender, receiver) = watch::channel(status(State::Connecting));
        let task = tokio::spawn(serve(path.clone(), receiver));

        let wait_for = |state: &'static str| {
            let path = path.clone();
            async move {
                for _ in 0..200 {
                    if let Ok(text) = read(&path) {
                        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                        if value["state"] == state {
                            return value;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                panic!("the status never became {state}");
            }
        };
        let value = wait_for("connecting").await;
        assert_eq!(value["node_id"], "office_pc");
        assert_eq!(value["v"], 1);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "any local user may read it");

        sender.send(status(State::Connected)).unwrap();
        wait_for("connected").await;
        drop(sender);
        task.await.unwrap();

        remove(&path);
        remove(&path);
        let error = read(&path).unwrap_err();
        assert!(error.contains("not running"), "{error}");
    }

    #[test]
    fn refuses_a_file_that_is_not_a_status() {
        let scratch = Scratch::new("not-status");
        let path = scratch.0.join("status.json");
        for text in ["", "[]", "not json", "{\"state\":"] {
            fs::write(&path, text).unwrap();
            assert!(read(&path).is_err(), "{text}");
        }
        fs::write(&path, "x".repeat(5000)).unwrap();
        assert!(read(&path).is_err());
        fs::write(&path, status(State::Waiting).line()).unwrap();
        assert!(read(&path).unwrap().starts_with('{'));
    }
}
