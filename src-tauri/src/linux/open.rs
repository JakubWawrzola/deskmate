//! Opening a web address in the user's default browser with `xdg-open`.
//! The caller has checked the address (`sys_commands::parse_web_url`): an
//! absolute http or https URL, so it can never be read as an option.

use std::process::{Command, Stdio};

use super::find_program;

/// Whether `xdg-open` is installed.
pub fn available() -> bool {
    find_program("xdg-open").is_some()
}

/// Hands a checked `http://` or `https://` address to `xdg-open`.
pub fn web_url(checked: &str) -> Result<(), String> {
    let url = url::Url::parse(checked).map_err(|e| format!("open_url: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.as_str().starts_with('-') {
        return Err("only http and https addresses are opened".into());
    }
    let program = find_program("xdg-open").ok_or("xdg-open is not installed")?;
    let mut child = Command::new(program)
        .arg(url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start xdg-open: {e}"))?;
    // Some desktops keep xdg-open running until the browser is closed; it is
    // reaped in the background instead of being waited for here.
    std::thread::spawn(move || {
        if let Err(error) = child.wait() {
            log::warn!("xdg-open: {error}");
        }
    });
    Ok(())
}
