//! Local security helpers. The audit log records event metadata and Link Files
//! paths, never file contents, clipboard contents, URLs, credentials or commands.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static AUDIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const MAX_AUDIT_BYTES: u64 = 1024 * 1024;

pub fn audit(event: &str, result: &str) {
    // Unit tests exercise audited code paths; they must not write into the
    // real security log of the developer's machine.
    if cfg!(test) {
        return;
    }
    let _guard = AUDIT_LOCK.get_or_init(|| Mutex::new(())).lock().ok();
    let Some(dir) = crate::config::config_path().parent().map(ToOwned::to_owned) else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("security.log");
    if fs::metadata(&path).map(|m| m.len() >= MAX_AUDIT_BYTES).unwrap_or(false) {
        let previous = dir.join("security.log.1");
        let _ = fs::remove_file(&previous);
        let _ = fs::rename(&path, previous);
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{timestamp} {event} {result}");
    }
}

pub fn safe_preview(text: &str, max_chars: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max_chars)
        .collect()
}

pub fn audit_file(op: &str, path: &str, result: &str) {
    let safe_op = safe_preview(op, 24).replace(' ', "_");
    let safe_path = safe_preview(path, 512);
    let safe_result = safe_preview(result, 160);
    audit("link_fs", &format!("op={safe_op} path={safe_path} result={safe_result}"));
}

/// A file sent to or read from this computer, for the Files page.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct FileEvent {
    /// Unix seconds.
    pub time: u64,
    /// received | read | cancelled
    pub kind: String,
    pub path: String,
    /// "ok" or why it was refused.
    pub result: String,
}

fn parse_file_event(line: &str) -> Option<FileEvent> {
    let (time, rest) = line.split_once(' ')?;
    let rest = rest.strip_prefix("link_fs op=")?;
    let (op, rest) = rest.split_once(" path=")?;
    let (path, result) = rest.rsplit_once(" result=")?;
    let kind = match op {
        "put_begin" if result != "ok" => "received",
        "put_end" => "received",
        "put_abort" => "cancelled",
        "read" => "read",
        _ => return None,
    };
    Some(FileEvent {
        time: time.parse().ok()?,
        kind: kind.into(),
        path: path.into(),
        result: result.into(),
    })
}

/// The latest file transfers from the security log, newest first. A file read
/// in many pieces shows once.
pub fn recent_file_events(limit: usize) -> Vec<FileEvent> {
    let Some(dir) = crate::config::config_path().parent().map(ToOwned::to_owned) else {
        return Vec::new();
    };
    let raw = fs::read_to_string(dir.join("security.log")).unwrap_or_default();
    let mut events: Vec<FileEvent> = Vec::new();
    for event in raw.lines().filter_map(parse_file_event) {
        if let Some(last) = events.last() {
            if last.kind == event.kind && last.path == event.path && last.result == event.result {
                continue;
            }
        }
        events.push(event);
    }
    events.reverse();
    events.truncate(limit);
    events
}

#[cfg(windows)]
pub fn confirm(title: &str, message: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_YESNO,
    };
    let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let message_w: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND,
        ) == IDYES
    }
}

#[cfg(not(windows))]
pub fn confirm(_title: &str, _message: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::parse_file_event;

    #[test]
    fn reads_file_events_from_the_log() {
        let event = parse_file_event("1760400000 link_fs op=put_end path=My photo.jpg result=ok").unwrap();
        assert_eq!(event.kind, "received");
        assert_eq!(event.path, "My photo.jpg");
        assert_eq!(event.result, "ok");
        let denied =
            parse_file_event("1760400000 link_fs op=put_begin path=a.exe result=denied: refused").unwrap();
        assert_eq!(denied.result, "denied: refused");
        assert!(parse_file_event("1760400000 link_fs op=list path=C:\\ result=ok").is_none());
        assert!(parse_file_event("1760400000 custom_command started").is_none());
    }
}
