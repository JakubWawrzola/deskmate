//! The daemon's log: lines on stderr, which systemd hands to the journal
//! (`journalctl -u deskmate`). Each line starts with its syslog priority in
//! angle brackets, which journald reads as the priority of the entry.
//!
//! As with the Windows service's log, callers never pass a key, a frame or
//! anything Home Assistant sent.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::io::Write;

struct Journal;

/// The syslog priority journald gives a line that starts with `<n>`.
fn priority(level: Level) -> u8 {
    match level {
        Level::Error => 3,
        Level::Warn => 4,
        Level::Info => 6,
        Level::Debug | Level::Trace => 7,
    }
}

/// One entry: the priority, the text, and no line break inside it, so that
/// one entry never turns into two.
fn line(level: Level, text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    format!("<{}>{flat}\n", priority(level))
}

impl Log for Journal {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let entry = line(record.level(), &record.args().to_string());
        // A log that cannot be written has nowhere to report that.
        let _ = std::io::stderr().lock().write_all(entry.as_bytes());
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// Sends everything logged through the `log` macros to stderr.
pub fn init() -> Result<(), String> {
    log::set_logger(&Journal).map_err(|e| e.to_string())?;
    log::set_max_level(LevelFilter::Info);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_carries_its_priority_and_stays_one_line() {
        assert_eq!(line(Level::Info, "runner started"), "<6>runner started\n");
        assert_eq!(line(Level::Warn, "w"), "<4>w\n");
        assert_eq!(line(Level::Error, "e"), "<3>e\n");
        assert_eq!(line(Level::Info, "first\nsecond\r"), "<6>first second \n");
    }
}
