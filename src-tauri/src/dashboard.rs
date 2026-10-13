//! Home Assistant dashboards (Lovelace) in a Deskmate window, from the tray,
//! a hotkey or a tray quick action.
//!
//! The window loads Home Assistant's own web interface. Login happens there
//! once and the session stays in this app's WebView2 profile. The page is a
//! remote site to Tauri: no capability grants it IPC, so Home Assistant's page
//! cannot call into Deskmate.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::config::AppConfig;
use crate::state::AppState;

const LABEL: &str = "dashboard";

/// Home Assistant's web address: the API URL from Settings, or else the one
/// Deskmate Link connects to (ws -> http, wss -> https, path dropped).
pub fn base_url(cfg: &AppConfig) -> Option<url::Url> {
    let candidates = [cfg.ha_url.trim(), cfg.link_url.trim()];
    for candidate in candidates.into_iter().filter(|c| !c.is_empty()) {
        let Ok(mut url) = url::Url::parse(candidate) else {
            continue;
        };
        let scheme = match url.scheme() {
            "http" | "ws" => "http",
            "https" | "wss" => "https",
            _ => continue,
        };
        if url.set_scheme(scheme).is_err() {
            continue;
        }
        url.set_path("/");
        url.set_query(None);
        url.set_fragment(None);
        return Some(url);
    }
    None
}

/// "lovelace/0", "/dashboard-pc/home" -> full URL on the Home Assistant host.
/// Only a path: a different host or scheme cannot be smuggled in.
pub fn dashboard_url(cfg: &AppConfig, path: &str) -> Result<url::Url, String> {
    let base = base_url(cfg).ok_or(
        "Home Assistant's address is unknown: set it under Settings > Home Assistant API, or connect with the Deskmate integration",
    )?;
    let path = path.trim().trim_start_matches('/');
    if path.contains("://") || path.starts_with("//") || path.contains('\\') {
        return Err("the dashboard must be a path such as lovelace/0".into());
    }
    let url = base.join(path).map_err(|e| e.to_string())?;
    if url.host_str() != base.host_str() || url.scheme() != base.scheme() {
        return Err("the dashboard must be on the Home Assistant host".into());
    }
    Ok(url)
}

/// Opens (or brings back) the dashboard window. `path` overrides the default
/// dashboard from Settings; empty uses it.
pub async fn open(app: &AppHandle, path: &str) -> Result<(), String> {
    let cfg = app.state::<AppState>().config.lock().await.clone();
    let path = if path.trim().is_empty() {
        cfg.dashboard_path.clone()
    } else {
        path.to_string()
    };
    let url = dashboard_url(&cfg, &path)?;
    if let Some(window) = app.get_webview_window(LABEL) {
        let same = window.url().map(|current| current.path() == url.path()).unwrap_or(false);
        if !same {
            window.navigate(url).map_err(|e| e.to_string())?;
        }
        let _ = window.set_always_on_top(cfg.dashboard_on_top);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("Home Assistant")
        .inner_size(1100.0, 760.0)
        .min_inner_size(420.0, 320.0)
        .always_on_top(cfg.dashboard_on_top)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_dashboard(app: AppHandle, path: Option<String>) -> Result<(), String> {
    open(&app, path.as_deref().unwrap_or("")).await
}

#[cfg(test)]
mod tests {
    use super::{base_url, dashboard_url};
    use crate::config::AppConfig;

    fn cfg(ha: &str, link: &str) -> AppConfig {
        AppConfig {
            ha_url: ha.into(),
            link_url: link.into(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn derives_the_web_address() {
        let c = cfg("", "wss://ha.example.com/api/deskmate_link/ws");
        assert_eq!(base_url(&c).unwrap().as_str(), "https://ha.example.com/");
        let c = cfg("http://192.168.1.5:8123", "ws://other:8123");
        assert_eq!(base_url(&c).unwrap().as_str(), "http://192.168.1.5:8123/");
        assert!(base_url(&cfg("", "")).is_none());
    }

    #[test]
    fn keeps_dashboards_on_the_same_host() {
        let c = cfg("http://ha.local:8123", "");
        assert_eq!(
            dashboard_url(&c, "/lovelace/0").unwrap().as_str(),
            "http://ha.local:8123/lovelace/0"
        );
        assert!(dashboard_url(&c, "https://evil.example/").is_err());
        assert!(dashboard_url(&c, "//evil.example/x").is_err());
    }
}
