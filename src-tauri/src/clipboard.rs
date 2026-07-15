//! PC <-> HA clipboard bridge (arboard; uses clipboard-win on Windows, no C).
//! Read = privacy sensor (opt-in). Write = text entity from HA.

use std::sync::{Mutex, OnceLock};

static LAST_WRITE: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();
const WRITE_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(2);

/// Claims the clipboard-write cooldown slot. This limits both accidental loops
/// and a hostile publisher without storing any clipboard contents.
pub fn claim_write_slot() -> bool {
    let Ok(mut last) = LAST_WRITE.get_or_init(|| Mutex::new(None)).lock() else {
        return false;
    };
    if last.map(|t| t.elapsed() < WRITE_COOLDOWN).unwrap_or(false) {
        return false;
    }
    *last = Some(std::time::Instant::now());
    true
}

/// Reads clipboard text (None if empty/image/error). One retry, since another
/// process may briefly hold the clipboard open.
pub fn get_text() -> Option<String> {
    for _ in 0..2 {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(t) = cb.get_text() {
                return Some(t);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    None
}

/// Sets the PC clipboard text (command from HA).
pub fn set_text(s: &str) -> Result<(), String> {
    let mut last = String::from("clipboard busy");
    for _ in 0..3 {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(s.to_string())) {
            Ok(_) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(last)
}
