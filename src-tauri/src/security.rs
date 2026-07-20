//! Local security helpers. The audit log records event metadata and Link Files
//! paths, never file contents, clipboard contents, URLs, credentials or commands.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static AUDIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const MAX_AUDIT_BYTES: u64 = 1024 * 1024;

pub fn audit(event: &str, result: &str) {
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
