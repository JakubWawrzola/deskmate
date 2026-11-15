//! The service's own log: `service.log` in the data folder, rotated by size.
//!
//! It records operations and their results. Callers never pass a key, a
//! frame or anything Home Assistant sent.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Name of the log in the data folder.
pub const FILE_NAME: &str = "service.log";
/// The log moves to `service.log.1` when it reaches this size.
const MAX_BYTES: u64 = 1024 * 1024;

struct Target {
    path: PathBuf,
    file: File,
    written: u64,
}

impl Target {
    fn open(path: PathBuf) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            path,
            file,
            written,
        })
    }

    fn write(&mut self, line: &str) -> std::io::Result<()> {
        if self.written >= MAX_BYTES {
            let mut previous = self.path.clone().into_os_string();
            previous.push(".1");
            let _ = fs::remove_file(&previous);
            fs::rename(&self.path, &previous)?;
            *self = Self::open(self.path.clone())?;
        }
        self.file.write_all(line.as_bytes())?;
        self.written += line.len() as u64;
        Ok(())
    }
}

struct FileLog {
    target: Mutex<Target>,
    /// Console runs show the same lines on stderr.
    echo: bool,
}

impl Log for FileLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let line = format!("{timestamp} {} {}\n", record.level(), record.args());
        if self.echo {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        let mut target = self.target.lock().unwrap_or_else(|e| e.into_inner());
        // A log that cannot be written has nowhere to report that.
        let _ = target.write(&line);
    }

    fn flush(&self) {
        let mut target = self.target.lock().unwrap_or_else(|e| e.into_inner());
        let _ = target.file.flush();
    }
}

/// Sends everything logged through the `log` macros to `service.log` in
/// `dir`. `echo` repeats the lines on stderr.
pub fn init(dir: &Path, echo: bool) -> Result<(), String> {
    let target = Target::open(dir.join(FILE_NAME))
        .map_err(|e| format!("cannot open the service log: {e}"))?;
    log::set_boxed_logger(Box::new(FileLog {
        target: Mutex::new(target),
        echo,
    }))
    .map_err(|e| e.to_string())?;
    log::set_max_level(LevelFilter::Info);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_log_moves_aside_and_a_new_one_starts() {
        let dir =
            std::env::temp_dir().join(format!("deskmate-service-log-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        let mut target = Target::open(path.clone()).unwrap();

        target.write("first line\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first line\n");

        // Pretend the file has grown to the limit.
        target.written = MAX_BYTES;
        target.write("second line\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second line\n");
        assert_eq!(
            fs::read_to_string(dir.join("service.log.1")).unwrap(),
            "first line\n"
        );
        assert_eq!(target.written, "second line\n".len() as u64);

        drop(target);
        fs::remove_dir_all(&dir).unwrap();
    }
}
