//! Daily check for a newer Deskmate release on GitHub.
//!
//! Reads the public "latest release" endpoint and nothing else: nothing is
//! downloaded or installed. A newer version shows up in the tray menu, on the
//! Status page and once as a notification, each pointing at the release page.
//! Off switch: Settings > Updates.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

const LATEST_API: &str = "https://api.github.com/repos/JakubWawrzola/deskmate/releases/latest";
const RELEASE_PAGE: &str = "https://github.com/JakubWawrzola/deskmate/releases/tag/v";
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
}

static AVAILABLE: Mutex<Option<UpdateInfo>> = Mutex::new(None);

/// The newer release found by the last check, if any.
pub fn available() -> Option<UpdateInfo> {
    AVAILABLE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// "v1.2.3", "1.2" or "1.2.3-beta" -> (1, 2, 3). Anything else -> None.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn fetch_latest_tag() -> Result<String, String> {
    let connector = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(connector))
        .timeout(Duration::from_secs(15))
        .build();
    let body = agent
        .get(LATEST_API)
        .set("User-Agent", concat!("Deskmate/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the release has no tag".to_string())
}

/// Asks GitHub now. Blocking; call it off the async runtime.
pub fn check_now() -> Result<Option<UpdateInfo>, String> {
    let tag = fetch_latest_tag()?;
    let latest = parse_version(&tag).ok_or_else(|| "unexpected release tag".to_string())?;
    let current = parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0));
    // Rebuilt from the parsed numbers: nothing from the response itself ends
    // up in the UI or in the link that gets opened.
    let info = (latest > current).then(|| {
        let version = format!("{}.{}.{}", latest.0, latest.1, latest.2);
        UpdateInfo {
            url: format!("{RELEASE_PAGE}{version}"),
            version,
        }
    });
    *AVAILABLE.lock().unwrap_or_else(|e| e.into_inner()) = info.clone();
    Ok(info)
}

/// Opens the release page of the update found by the last check.
pub fn open_release_page() -> Result<(), String> {
    let info = available().ok_or_else(|| "no update available".to_string())?;
    crate::sys_commands::open_url(&info.url)
}

/// Background loop: first check a minute after start, then once a day.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            let enabled = app.state::<AppState>().config.lock().await.update_check;
            if enabled {
                match tokio::task::spawn_blocking(check_now).await {
                    Ok(Ok(Some(info))) => announce(&app, &info).await,
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => log::info!("update check failed: {error}"),
                    Err(_) => {}
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Tray entry every time; a notification only once per version.
pub async fn announce(app: &AppHandle, info: &UpdateInfo) {
    let state = app.state::<AppState>();
    let (cfg, first_time) = {
        let mut cfg = state.config.lock().await;
        let first_time = cfg.update_notified != info.version;
        if first_time {
            cfg.update_notified = info.version.clone();
            let _ = crate::config::save(&cfg);
        }
        (cfg.clone(), first_time)
    };
    let _ = crate::rebuild_tray_menu(app, &cfg);
    if first_time {
        let toast = crate::notify::NotifyPayload {
            title: format!("Deskmate {} is available", info.version),
            message: "Download it from the tray menu or the Status page. Settings and pairing are kept.".into(),
            image: None,
            actions: Vec::new(),
        };
        let _ = tokio::task::spawn_blocking(move || crate::notify::show_toast(&toast)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn parses_release_tags() {
        assert_eq!(parse_version("v0.7.0"), Some((0, 7, 0)));
        assert_eq!(parse_version("0.8"), Some((0, 8, 0)));
        assert_eq!(parse_version("v1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
    }
}
