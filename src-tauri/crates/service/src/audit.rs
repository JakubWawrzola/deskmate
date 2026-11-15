//! The service's audit log: `security.log` in the data folder.
//!
//! One line per security decision: a Unix timestamp, the event and its
//! result. Never a key, a frame or anything Home Assistant sent; the callers
//! pass fixed words and values they have checked against a fixed list. The
//! format is the one the app uses for its own `security.log`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Name of the audit log in the data folder.
pub const FILE_NAME: &str = "security.log";
/// The log moves to `security.log.1` when it reaches this size.
const MAX_BYTES: u64 = 1024 * 1024;

/// Where security decisions are recorded.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: &str, result: &str);
}

/// The audit log on disk.
pub struct AuditFile {
    path: PathBuf,
    lock: Mutex<()>,
}

impl AuditFile {
    pub fn new(dir: &std::path::Path) -> Self {
        Self {
            path: dir.join(FILE_NAME),
            lock: Mutex::new(()),
        }
    }
}

impl AuditSink for AuditFile {
    fn record(&self, event: &str, result: &str) {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if fs::metadata(&self.path).is_ok_and(|metadata| metadata.len() >= MAX_BYTES) {
            let mut previous = self.path.clone().into_os_string();
            previous.push(".1");
            let _ = fs::remove_file(&previous);
            let _ = fs::rename(&self.path, &previous);
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                if let Err(error) = writeln!(file, "{timestamp} {event} {result}") {
                    log::error!("cannot write the audit log: {error}");
                }
            }
            Err(error) => log::error!("cannot open the audit log: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_one_line_per_event() {
        let dir = std::env::temp_dir().join(format!(
            "deskmate-service-audit-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let audit = AuditFile::new(&dir);
        audit.record("power_command", "key=restart executor=dry_run result=ok");
        audit.record("provision", "ok");

        let text = fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let (timestamp, rest) = lines[0].split_once(' ').unwrap();
        assert!(timestamp.parse::<u64>().unwrap() > 1_700_000_000);
        assert_eq!(rest, "power_command key=restart executor=dry_run result=ok");
        assert!(lines[1].ends_with(" provision ok"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
