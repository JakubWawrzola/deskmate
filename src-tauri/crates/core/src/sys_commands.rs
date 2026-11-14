//! Command descriptors, the power commands and the URL checks: the part of
//! the system commands that needs no signed-in user. The commands that act on
//! the user's session (lock, media keys, volume, typed text, custom commands)
//! are in the app's `sys_commands.rs`.
//! SECURITY: a payload received from Home Assistant is NEVER executed. Only
//! predefined actions run, looked up by key.

use std::process::Command;
use url::Url;

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandDef {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
}

pub const COMMAND_DEFS: &[CommandDef] = &[
    CommandDef {
        id: "lock",
        name: "Lock",
        icon: "mdi:lock",
    },
    CommandDef {
        id: "sleep",
        name: "Sleep",
        icon: "mdi:power-sleep",
    },
    CommandDef {
        id: "hibernate",
        name: "Hibernate",
        icon: "mdi:power-sleep",
    },
    CommandDef {
        id: "shutdown",
        name: "Shutdown",
        icon: "mdi:power",
    },
    CommandDef {
        id: "restart",
        name: "Restart",
        icon: "mdi:restart",
    },
    CommandDef {
        id: "sign_out",
        name: "Sign out",
        icon: "mdi:logout",
    },
    CommandDef {
        id: "monitor_off",
        name: "Monitors off",
        icon: "mdi:monitor-off",
    },
    CommandDef {
        id: "media_play_pause",
        name: "Media play/pause",
        icon: "mdi:play-pause",
    },
    CommandDef {
        id: "media_next",
        name: "Media next",
        icon: "mdi:skip-next",
    },
    CommandDef {
        id: "media_prev",
        name: "Media previous",
        icon: "mdi:skip-previous",
    },
    CommandDef {
        id: "empty_recycle_bin",
        name: "Empty recycle bin",
        icon: "mdi:delete-empty",
    },
];

/// Presentation commands - shown in discovery ONLY when allow_input=true.
pub const PRESENT_DEFS: &[CommandDef] = &[
    CommandDef {
        id: "present_next",
        name: "Presentation next",
        icon: "mdi:arrow-right-bold",
    },
    CommandDef {
        id: "present_prev",
        name: "Presentation previous",
        icon: "mdi:arrow-left-bold",
    },
    CommandDef {
        id: "present_start",
        name: "Presentation start",
        icon: "mdi:play-box",
    },
    CommandDef {
        id: "present_black",
        name: "Presentation black",
        icon: "mdi:square",
    },
    CommandDef {
        id: "present_end",
        name: "Presentation end",
        icon: "mdi:stop",
    },
];

/// Keys of the commands `run_power` knows.
pub const POWER_COMMANDS: [&str; 4] = ["sleep", "hibernate", "shutdown", "restart"];

/// Runs one of the power commands. They act on the whole computer, so they
/// work without a signed-in user. Returns Err for any other key.
pub fn run_power(key: &str) -> Result<(), String> {
    match key {
        "sleep" => spawn("rundll32", &["powrprof.dll,SetSuspendState", "0,1,0"]),
        "hibernate" => spawn("shutdown", &["/h"]),
        "shutdown" => spawn("shutdown", &["/s", "/t", "5"]),
        "restart" => spawn("shutdown", &["/r", "/t", "5"]),
        _ => Err(format!("unknown power command: {key}")),
    }
}

fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Parses a web URL accepted from MQTT. This deliberately does not decide which
/// hosts are trusted: blocking private addresses would break normal local HA
/// dashboards. Broker authentication/ACLs are the authorization boundary.
pub fn parse_web_url(input: &str) -> Result<Url, String> {
    let value = input.trim();
    if value.len() > 2048 || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("URL contains invalid characters".into());
    }
    let url = Url::parse(value).map_err(|_| "invalid URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
        return Err("only absolute http/https URLs are allowed".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with embedded credentials are not allowed".into());
    }
    Ok(url)
}

/// Normalizes an exact HTTP(S) origin used by the URL allowlist.
pub fn normalize_allowed_origin(input: &str) -> Result<String, String> {
    let url = parse_web_url(input)?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err("allowed URL entries must be origins without a path, query or fragment".into());
    }
    Ok(url.origin().ascii_serialization())
}

pub fn parse_allowed_web_url(cfg: &crate::config::AppConfig, input: &str) -> Result<Url, String> {
    let url = parse_web_url(input)?;
    let origin = url.origin().ascii_serialization();
    let explicitly_allowed = cfg
        .allowed_url_origins
        .iter()
        .filter_map(|entry| normalize_allowed_origin(entry).ok())
        .any(|allowed| allowed == origin);
    let ha_allowed = [&cfg.ha_url, &cfg.ha_url_remote]
        .into_iter()
        .filter(|entry| !entry.trim().is_empty())
        .filter_map(|entry| parse_web_url(entry).ok())
        .any(|entry| entry.origin().ascii_serialization() == origin);
    if !explicitly_allowed && !ha_allowed {
        return Err(format!("URL origin is not allowed: {origin}"));
    }
    Ok(url)
}
