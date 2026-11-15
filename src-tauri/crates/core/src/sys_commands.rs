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
        id: "wake_screen",
        name: "Wake screen",
        icon: "mdi:monitor",
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
    let (program, args) =
        power_command(key).ok_or_else(|| format!("unknown power command: {key}"))?;
    spawn(program, args)
}

/// A program and its arguments.
type FixedCommand = (&'static str, &'static [&'static str]);

/// The command line behind each power command. It is fixed: the key only
/// picks one, and nothing received over the network becomes part of it.
#[cfg(windows)]
fn power_command(key: &str) -> Option<FixedCommand> {
    match key {
        "sleep" => Some(("rundll32", &["powrprof.dll,SetSuspendState", "0,1,0"])),
        "hibernate" => Some(("shutdown", &["/h"])),
        "shutdown" => Some(("shutdown", &["/s", "/t", "5"])),
        "restart" => Some(("shutdown", &["/r", "/t", "5"])),
        _ => None,
    }
}

/// On Linux systemd carries them out. It asks logind, which decides whether
/// the user this program runs as may do it.
#[cfg(target_os = "linux")]
fn power_command(key: &str) -> Option<FixedCommand> {
    const SYSTEMCTL: &str = "systemctl";
    match key {
        "sleep" => Some((SYSTEMCTL, &["suspend"])),
        "hibernate" => Some((SYSTEMCTL, &["hibernate"])),
        "shutdown" => Some((SYSTEMCTL, &["poweroff"])),
        "restart" => Some((SYSTEMCTL, &["reboot"])),
        _ => None,
    }
}

fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    let child = Command::new(program)
        .args(args)
        .spawn()
        .map_err(|e| e.to_string())?;
    reap(child);
    Ok(())
}

/// Windows forgets a finished child once its handle is closed.
#[cfg(windows)]
fn reap(_child: std::process::Child) {}

/// On Linux a finished child stays in the process table until its parent
/// asks how it ended. A thread waits for it, so that a daemon running for
/// months collects no dead entries, and the log says when the command failed.
#[cfg(target_os = "linux")]
fn reap(mut child: std::process::Child) {
    let waiting = std::thread::Builder::new()
        .name("power-command".into())
        .spawn(move || match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => log::warn!("the power command failed: {status}"),
            Err(error) => log::warn!("cannot tell how the power command ended: {error}"),
        });
    if let Err(error) = waiting {
        log::warn!("cannot wait for the power command: {error}");
    }
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

#[cfg(test)]
mod tests {
    use super::{power_command, run_power, POWER_COMMANDS};

    // The commands themselves are never run in a test: they would put the
    // computer to sleep or turn it off. The tests cover which command a key
    // stands for.

    #[cfg(windows)]
    #[test]
    fn power_keys_stand_for_the_windows_commands() {
        let args = |key| power_command(key).map(|(program, args)| (program, args.to_vec()));
        assert_eq!(
            args("sleep"),
            Some(("rundll32", vec!["powrprof.dll,SetSuspendState", "0,1,0"]))
        );
        assert_eq!(args("hibernate"), Some(("shutdown", vec!["/h"])));
        assert_eq!(args("shutdown"), Some(("shutdown", vec!["/s", "/t", "5"])));
        assert_eq!(args("restart"), Some(("shutdown", vec!["/r", "/t", "5"])));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn power_keys_stand_for_the_systemctl_commands() {
        let args = |key| power_command(key).map(|(program, args)| (program, args.to_vec()));
        assert_eq!(args("sleep"), Some(("systemctl", vec!["suspend"])));
        assert_eq!(args("hibernate"), Some(("systemctl", vec!["hibernate"])));
        assert_eq!(args("shutdown"), Some(("systemctl", vec!["poweroff"])));
        assert_eq!(args("restart"), Some(("systemctl", vec!["reboot"])));
    }

    #[test]
    fn only_the_four_power_keys_stand_for_a_command() {
        for key in POWER_COMMANDS {
            assert!(power_command(key).is_some(), "{key}");
        }
        for key in [
            "",
            "lock",
            "Sleep",
            "sleep ",
            "sleep;reboot",
            "suspend",
            "poweroff",
            "reboot",
            "restart --force",
            "custom_backup",
        ] {
            assert!(power_command(key).is_none(), "{key}");
            assert!(run_power(key).is_err(), "{key}");
        }
    }
}
