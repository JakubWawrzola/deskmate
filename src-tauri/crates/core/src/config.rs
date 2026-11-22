//! Application configuration: config.json in the configuration folder,
//! %APPDATA%\Deskmate on Windows and the XDG folders on Linux.
//! Secrets NEVER land in JSON - they are kept in the selected secret store
//! (secrets.rs), Windows Credential Manager by default.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::consts;
use crate::secrets;

fn default_kind() -> String {
    "button".into()
}
fn default_num_max() -> f64 {
    100.0
}
fn default_num_step() -> f64 {
    1.0
}
fn default_mqtt_transport() -> String {
    "tls".into()
}
/// Configs written before 0.5 have no `transport` field and always used MQTT.
/// A missing field therefore still means MQTT, so those setups keep working.
fn legacy_transport() -> String {
    "mqtt".into()
}
fn default_clipboard_mode() -> String {
    "off".into()
}
fn default_files_max_mb() -> u64 {
    256
}
/// Configs written before automatic updates existed have no `update_mode`.
/// They get the default: Deskmate asks before it installs anything.
fn default_update_mode() -> String {
    "ask".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomCommand {
    /// identifier [a-z0-9_], part of the entity's object_id in HA
    pub id: String,
    /// name visible in HA
    pub name: String,
    /// program + arguments executed via PowerShell (NOT from the MQTT payload!)
    /// The control's value (switch on/off, number) is passed to the script as the
    /// $env:DESKMATE_VALUE environment variable (no interpolation into code - anti-RCE).
    pub command: String,
    /// control type in HA: button | switch | number
    #[serde(default = "default_kind")]
    pub kind: String,
    /// range for number
    #[serde(default)]
    pub num_min: f64,
    #[serde(default = "default_num_max")]
    pub num_max: f64,
    #[serde(default = "default_num_step")]
    pub num_step: f64,
    /// Disabled commands are not exposed through MQTT discovery and cannot run.
    #[serde(default)]
    pub enabled: bool,
    /// Ask on the Windows desktop before each execution.
    #[serde(default = "default_true")]
    pub require_confirmation: bool,
    /// powershell: `command` is a script | launch: `command` is a program, file
    /// or URL opened with `args` | stop: `command` is a program name to stop |
    /// keys: `command` is a key combination such as "Ctrl+F5" (keys.rs).
    /// launch, stop and keys never go through PowerShell.
    #[serde(default = "default_command_action")]
    pub action: String,
    /// Arguments for launch, passed as typed.
    #[serde(default)]
    pub args: String,
}

fn default_command_action() -> String {
    "powershell".into()
}

/// Most programs a user may list in `AppConfig::game_programs`.
pub const MAX_GAME_PROGRAMS: usize = 40;

/// A sensor added on the Sensors page (custom_sensors.rs).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomSensor {
    /// [a-z0-9_]
    pub id: String,
    pub name: String,
    /// process | window | service | powershell | monitor
    pub kind: String,
    /// program name | window title text | service name | PowerShell command |
    /// the identifier of a sensor of LibreHardwareMonitor
    pub target: String,
    /// optional unit for a PowerShell sensor that prints a number; the unit
    /// of a monitor sensor
    #[serde(default)]
    pub unit: String,
    /// PowerShell only: seconds between runs (30 or more)
    #[serde(default = "default_sensor_interval")]
    pub interval_secs: u64,
    #[serde(default)]
    pub enabled: bool,
}

fn default_sensor_interval() -> u64 {
    60
}

impl Default for CustomCommand {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            command: String::new(),
            kind: default_kind(),
            num_min: 0.0,
            num_max: default_num_max(),
            num_step: default_num_step(),
            enabled: false,
            require_confirmation: true,
            action: default_command_action(),
            args: String::new(),
        }
    }
}

/// Action shared by hotkeys and tray quick actions.
/// kind: toggle | service | command | widget | mqtt
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ActionSpec {
    pub kind: String,
    /// for toggle/service: HA entity (e.g. light.kanapa)
    pub entity_id: String,
    /// for service: "domain.service" (e.g. scene.turn_on)
    pub service: String,
    /// for service: additional JSON fields (string, validated at execution time)
    pub data: String,
    /// for command: builtin command id or "custom_<id>"
    pub command_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Hotkey {
    /// identifier [a-z0-9_] (also used in the MQTT device trigger topic)
    pub id: String,
    pub name: String,
    /// accelerator e.g. "Ctrl+Alt+L", "Ctrl+Shift+F1"
    pub accelerator: String,
    pub action: ActionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TrayAction {
    pub id: String,
    pub name: String,
    pub action: ActionSpec,
}

/// Screen time and family rules (family.rs). Changed from Home Assistant or
/// the Family page, never by a stale copy the settings page sends back.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct FamilyConfig {
    /// Count screen time for this Windows account and show the family entities.
    /// An administrator's protection turns it on regardless (family.rs).
    pub enabled: bool,
    /// Enforce the daily limit and the allowed hours.
    pub rules_on: bool,
    /// Minutes per day, 0 = no limit.
    pub daily_limit_min: u32,
    /// "HH:MM" or empty.
    pub allowed_from: String,
    pub allowed_until: String,
    /// Close the programs below as soon as they start.
    pub block_apps: bool,
    /// Executable names such as "steam.exe".
    pub blocked_apps: Vec<String>,
    /// lock (default) | sign_out
    pub time_up_action: String,
    /// Minutes before the stop at which a warning is shown, earliest first.
    /// Empty = the built-in times (family/warnings.rs).
    pub warn_at_min: Vec<u32>,
}

/// An update install that was started. Written before the installer runs,
/// because the installer ends this process, and settled at the next start.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct UpdateAttempt {
    pub from: String,
    pub to: String,
    /// Started by the `auto` mode, not by a click.
    pub auto: bool,
    /// The window was hidden when the install started, so the restarted app
    /// stays in the tray as well.
    pub hidden: bool,
    /// Unix seconds.
    pub started_at: u64,
}

/// What the updater remembers between runs (updates.rs). Changed only by the
/// app itself, never by a stale copy the settings page sends back.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct UpdateState {
    /// "Later": do not ask again before this time, in Unix seconds.
    pub later_until: u64,
    pub installing: Option<UpdateAttempt>,
    /// The version whose install did not go through. The `auto` mode does not
    /// try that version again by itself.
    pub failed_version: String,
}

/// Widget panel tile.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WidgetItem {
    pub entity_id: String,
    /// tile label (empty = friendly_name from HA)
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub configured: bool,
    /// mqtt (default) | link
    #[serde(default = "legacy_transport")]
    pub transport: String,
    pub broker_host: String,
    /// optional fallback host (e.g. public IP / Tailscale). When set,
    /// the client first tries broker_host (local), and after a failed connection
    /// switches to this host. Empty = single host (as before).
    pub broker_host_remote: String,
    pub broker_port: u16,
    /// tls (default, certificate verified by Windows/custom CA) | insecure
    #[serde(default = "default_mqtt_transport")]
    pub mqtt_transport: String,
    /// Optional PEM CA certificate for a self-signed/private MQTT broker.
    pub mqtt_ca_path: String,
    pub username: String,
    /// Deskmate Link websocket endpoint. The API path is appended when omitted.
    pub link_url: String,
    /// Optional fallback Deskmate Link endpoint.
    pub link_url_remote: String,
    /// Absolute local directories exposed read-only through Link Files.
    pub link_file_roots: Vec<String>,
    /// off | confirm | automatic. Files sent from Home Assistant to this PC.
    #[serde(default = "default_clipboard_mode")]
    pub link_inbox_mode: String,
    /// Folder for received files. Empty = %USERPROFILE%\Downloads\Deskmate.
    pub link_inbox_dir: String,
    /// Largest file received or served over Link Files, in MiB.
    #[serde(default = "default_files_max_mb")]
    pub link_files_max_mb: u64,
    /// Opt-in second encryption layer for Link (ChaCha20-Poly1305 over
    /// AES-256-GCM, keyed separately). Must be enabled on both ends.
    #[serde(default)]
    pub link_cascade: bool,
    /// friendly device name in HA (default: hostname)
    pub device_name: String,
    /// id used in topics/unique_id: [a-z0-9_], default: sanitized hostname
    pub node_id: String,
    pub publish_interval_secs: u64,
    /// sensors that can be disabled; key = sensor id, missing entry = that sensor's default
    pub sensors_enabled: std::collections::HashMap<String, bool>,
    pub custom_commands: Vec<CustomCommand>,
    #[serde(default)]
    pub custom_sensors: Vec<CustomSensor>,
    /// Programs the user state counts as games while they fill the screen,
    /// as executable names such as "eldenring.exe" (user_state.rs).
    #[serde(default)]
    pub game_programs: Vec<String>,
    pub launch_hidden: bool,
    /// opt-in: allow HA to type text / control a presentation (SendInput keystrokes)
    pub allow_input: bool,
    /// opt-in: allow HA to make the computer speak (TTS)
    pub tts_enabled: bool,
    /// off | confirm | automatic. Controls periodic clipboard publication to HA.
    #[serde(default = "default_clipboard_mode")]
    pub clipboard_read_mode: String,
    /// off | confirm | automatic. Controls clipboard replacement requested by HA.
    #[serde(default = "default_clipboard_mode")]
    pub clipboard_write_mode: String,
    /// Exact allowed URL origins, e.g. https://example.com or http://ha.local:8123.
    /// Configured HA API origins are allowed automatically.
    pub allowed_url_origins: Vec<String>,
    /// brand toasts as "Deskmate" (AUMID + Start Menu shortcut). When false or
    /// when branding fails -> toasts carry the PowerShell AUMID (always works).
    #[serde(default = "default_true")]
    pub toast_branding: bool,
    /// "Start with Windows" as last chosen in Settings. None: not recorded yet
    /// (configs from before 0.8.0). Lets a Run entry removed by an update be
    /// restored (autostart.rs, GitHub issue #7).
    #[serde(default)]
    pub start_with_windows: Option<bool>,
    /// Home Assistant dashboard opened from the tray or a hotkey, as a path
    /// such as "lovelace/0". Empty: the user's default dashboard.
    #[serde(default)]
    pub dashboard_path: String,
    /// Keep the dashboard window above other windows.
    #[serde(default)]
    pub dashboard_on_top: bool,
    /// App colours: system (follow Windows) | light | dark.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Daily check for a newer release on GitHub. Off: Deskmate never looks
    /// for, downloads or installs an update by itself.
    #[serde(default = "default_true")]
    pub update_check: bool,
    /// What happens when the check finds a newer release: auto (install while
    /// the computer is idle) | ask (default) | notify (tell only).
    #[serde(default = "default_update_mode")]
    pub update_mode: String,
    /// Last version announced with a notification, so each is announced once.
    #[serde(default)]
    pub update_notified: String,
    #[serde(default)]
    pub update_state: UpdateState,
    /// Home Assistant URL (local), e.g. http://192.168.1.10:8123. Empty = API channel disabled.
    pub ha_url: String,
    /// fallback URL (Tailscale/public) - failover like the MQTT broker
    pub ha_url_remote: String,
    /// global keyboard shortcuts (work in the background)
    pub hotkeys: Vec<Hotkey>,
    /// widget panel tiles
    pub widgets: Vec<WidgetItem>,
    /// quick actions in the tray menu
    pub tray_actions: Vec<TrayAction>,
    /// screen time and family rules
    #[serde(default)]
    pub family: FamilyConfig,
    /// "Disconnect" on the Status page or in the tray: no connection to Home
    /// Assistant until the user connects again, also after a restart.
    #[serde(default)]
    pub connection_paused: bool,
    /// The local channel for the Stream Deck plugin. Off: the app opens no
    /// pipe, and programs on this computer cannot reach Home Assistant
    /// through it.
    #[serde(default)]
    pub streamdeck_enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "system".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        let host = hostname();
        Self {
            configured: false,
            // New installs start on the Deskmate integration; MQTT stays available.
            transport: "link".into(),
            broker_host: String::new(),
            broker_host_remote: String::new(),
            broker_port: 8883,
            mqtt_transport: default_mqtt_transport(),
            mqtt_ca_path: String::new(),
            username: String::new(),
            link_url: String::new(),
            link_url_remote: String::new(),
            link_file_roots: Vec::new(),
            link_inbox_mode: default_clipboard_mode(),
            link_inbox_dir: String::new(),
            link_files_max_mb: default_files_max_mb(),
            link_cascade: false,
            device_name: host.clone(),
            node_id: sanitize_id(&host),
            publish_interval_secs: 15,
            sensors_enabled: Default::default(),
            custom_commands: Vec::new(),
            custom_sensors: Vec::new(),
            game_programs: Vec::new(),
            launch_hidden: false,
            allow_input: false,
            tts_enabled: false,
            clipboard_read_mode: default_clipboard_mode(),
            clipboard_write_mode: default_clipboard_mode(),
            allowed_url_origins: Vec::new(),
            toast_branding: true,
            update_check: true,
            start_with_windows: None,
            theme: default_theme(),
            dashboard_path: String::new(),
            dashboard_on_top: false,
            update_mode: default_update_mode(),
            update_notified: String::new(),
            update_state: UpdateState::default(),
            ha_url: String::new(),
            ha_url_remote: String::new(),
            hotkeys: Vec::new(),
            widgets: Vec::new(),
            tray_actions: Vec::new(),
            family: FamilyConfig::default(),
            connection_paused: false,
            streamdeck_enabled: false,
        }
    }
}

impl AppConfig {
    /// Sets one of the switches that Settings stores the moment it is toggled,
    /// without the Save button.
    pub fn set_feature_flag(&mut self, flag: &str, enabled: bool) -> Result<(), String> {
        match flag {
            "allow_input" => self.allow_input = enabled,
            "tts_enabled" => self.tts_enabled = enabled,
            "toast_branding" => self.toast_branding = enabled,
            "update_check" => self.update_check = enabled,
            "launch_hidden" => self.launch_hidden = enabled,
            _ => return Err(format!("unknown flag: {flag}")),
        }
        Ok(())
    }
}

/// Stands in for a computer name the system does not report.
#[cfg(windows)]
const FALLBACK_HOST: &str = "windows-pc";
#[cfg(not(windows))]
const FALLBACK_HOST: &str = "linux-pc";

/// Stands in for a name without a single usable character.
#[cfg(windows)]
const FALLBACK_ID: &str = "windows_pc";
#[cfg(not(windows))]
const FALLBACK_ID: &str = "linux_pc";

pub fn hostname() -> String {
    sysinfo::System::host_name().unwrap_or_else(|_| FALLBACK_HOST.into())
}

/// hostname -> safe topic/unique_id id
pub fn sanitize_id(s: &str) -> String {
    let out: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        FALLBACK_ID.into()
    } else {
        trimmed.into()
    }
}

/// The folders a program keeps its files in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// Holds config.json.
    pub config: PathBuf,
    /// Holds what the program writes on its own: the security log and, for a
    /// program that keeps its secrets in files, the key files.
    pub data: PathBuf,
}

impl Dirs {
    /// The folders of a program that runs for the whole computer instead of
    /// one user, such as the daemon.
    #[cfg(target_os = "linux")]
    pub fn system() -> Self {
        Self {
            config: PathBuf::from("/etc").join(consts::CONFIG_DIR),
            data: PathBuf::from("/var/lib").join(consts::CONFIG_DIR),
        }
    }
}

static DIRS_OVERRIDE: OnceLock<Dirs> = OnceLock::new();

/// Replaces the per-user folders for this process. A program that runs
/// without a user calls it once at startup, before anything reads the
/// configuration. Fails when the folders were replaced before.
pub fn set_dirs(dirs: Dirs) -> Result<(), String> {
    DIRS_OVERRIDE
        .set(dirs)
        .map_err(|_| "the configuration and data folders were already set".to_string())
}

/// The folders in use: the ones given to `set_dirs`, otherwise the user's.
pub fn dirs() -> Dirs {
    DIRS_OVERRIDE.get().cloned().unwrap_or_else(user_dirs)
}

/// %APPDATA%\Deskmate for both: on Windows the security log has always been
/// next to config.json.
#[cfg(windows)]
fn user_dirs() -> Dirs {
    roaming_dirs(std::env::var("APPDATA").ok().map(PathBuf::from))
}

#[cfg(windows)]
fn roaming_dirs(appdata: Option<PathBuf>) -> Dirs {
    let dir = appdata
        .unwrap_or_else(|| PathBuf::from("."))
        .join(consts::CONFIG_DIR);
    Dirs {
        config: dir.clone(),
        data: dir,
    }
}

#[cfg(not(windows))]
fn user_dirs() -> Dirs {
    xdg_dirs(|name| std::env::var_os(name))
}

/// The XDG base directories: $XDG_CONFIG_HOME and $XDG_DATA_HOME, or
/// ~/.config and ~/.local/share. A value that is not an absolute path is
/// ignored, as the specification says.
#[cfg(not(windows))]
fn xdg_dirs(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> Dirs {
    let absolute = |name: &str| {
        env(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let home = absolute("HOME");
    let base = |variable: &str, under_home: &str| {
        absolute(variable)
            .or_else(|| home.as_ref().map(|home| home.join(under_home)))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(consts::CONFIG_DIR)
    };
    Dirs {
        config: base("XDG_CONFIG_HOME", ".config"),
        data: base("XDG_DATA_HOME", ".local/share"),
    }
}

pub fn config_path() -> PathBuf {
    dirs().config.join("config.json")
}

/// The folder for the security log and the key files.
pub fn data_dir() -> PathBuf {
    dirs().data
}

pub fn load() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let legacy_without_transport = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| {
                    value
                        .as_object()
                        .map(|object| !object.contains_key("mqtt_transport"))
                })
                .unwrap_or(false);
            let mut cfg = serde_json::from_str::<AppConfig>(&raw).unwrap_or_default();
            if legacy_without_transport {
                cfg.mqtt_transport = default_mqtt_transport();
                if cfg.broker_port == 1883 {
                    cfg.broker_port = 8883;
                }
            }
            cfg
        }
        Err(_) => AppConfig::default(),
    }
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    // atomic write: tmp + rename
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Reads a secret from the selected store. A store that cannot be read counts
/// as "no secret", and the log says why.
fn read_secret(service: &str, account: &str) -> Option<String> {
    match secrets::selected().get(service, account) {
        Ok(secret) => secret.map(|secret| secret.as_str().to_owned()),
        Err(error) => {
            log::warn!("cannot read a secret ({service}): {error}");
            None
        }
    }
}

/// Stores a secret in the selected store. An empty one removes the entry.
fn store_secret(service: &str, account: &str, secret: &str) -> Result<(), String> {
    if secret.is_empty() {
        forget_secret(service, account);
        return Ok(());
    }
    secrets::selected().set(service, account, secret)
}

/// Removing is a cleanup: a store that cannot do it leaves a line in the log
/// and does not fail what the user asked for.
fn forget_secret(service: &str, account: &str) {
    if let Err(error) = secrets::selected().delete(service, account) {
        log::warn!("cannot remove a secret ({service}): {error}");
    }
}

fn mqtt_account(username: &str, broker_host: &str) -> String {
    format!("{}@{}", username, broker_host)
}

pub fn set_password(cfg: &AppConfig, password: &str) -> Result<(), String> {
    let account = mqtt_account(&cfg.username, &cfg.broker_host);
    store_secret(consts::KEYRING_SERVICE, &account, password)
}

pub fn get_password(cfg: &AppConfig) -> Option<String> {
    let account = mqtt_account(&cfg.username, &cfg.broker_host);
    read_secret(consts::KEYRING_SERVICE, &account)
}

/// Removes the stored password of the old user@host (when the broker
/// changes), so it doesn't stay orphaned.
pub fn delete_password_for(username: &str, broker_host: &str) {
    let account = mqtt_account(username, broker_host);
    forget_secret(consts::KEYRING_SERVICE, &account);
}

pub fn set_link_key(node_id: &str, pairing_key: &str) -> Result<(), String> {
    store_secret(consts::LINK_KEYRING_SERVICE, node_id, pairing_key)
}

pub fn get_link_key(node_id: &str) -> Option<String> {
    read_secret(consts::LINK_KEYRING_SERVICE, node_id)
}

/// The cascade key lives in its own entry of the secret store, never in
/// config.json, and never next to the primary pairing key.
pub fn set_link_cascade_key(node_id: &str, key: &str) -> Result<(), String> {
    store_secret(consts::CASCADE_KEYRING_SERVICE, node_id, key)
}

pub fn get_link_cascade_key(node_id: &str) -> Option<String> {
    read_secret(consts::CASCADE_KEYRING_SERVICE, node_id)
}

/// Moves one secret from the `old` to the `new` account name. The target is
/// only written when it is still empty, so a key the user just pasted is
/// never overwritten by the one it replaces.
fn move_secret(service: &str, old: &str, new: &str) -> Result<(), String> {
    let store = secrets::selected();
    let Ok(Some(secret)) = store.get(service, old) else {
        return Ok(());
    };
    if !matches!(store.get(service, new), Ok(Some(_))) {
        store.set(service, new, &secret)?;
    }
    forget_secret(service, old);
    Ok(())
}

/// Renaming the node used to delete the Link key and silently orphan the
/// cascade key and the Home Assistant token, which are all stored per node id.
/// They now follow the node to its new name.
pub fn migrate_node_secrets(old: &str, new: &str) -> Result<(), String> {
    if old == new || old.is_empty() {
        return Ok(());
    }
    for service in [
        consts::LINK_KEYRING_SERVICE,
        consts::CASCADE_KEYRING_SERVICE,
        consts::HA_TOKEN_KEYRING_SERVICE,
    ] {
        move_secret(service, old, new)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AppConfig;
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn windows_keeps_everything_in_one_roaming_folder() {
        let dirs = super::roaming_dirs(Some(PathBuf::from(r"C:\Users\me\AppData\Roaming")));
        assert_eq!(
            dirs.config,
            PathBuf::from(r"C:\Users\me\AppData\Roaming\Deskmate")
        );
        assert_eq!(dirs.data, dirs.config);
        assert_eq!(
            super::roaming_dirs(None).config,
            PathBuf::from(".").join("Deskmate")
        );
    }

    #[cfg(windows)]
    #[test]
    fn config_file_is_in_the_configuration_folder() {
        let path = super::config_path();
        assert!(path.ends_with(r"Deskmate\config.json"));
        assert_eq!(path.parent(), Some(super::data_dir().as_path()));
    }

    #[cfg(not(windows))]
    fn xdg(vars: &[(&str, &str)]) -> super::Dirs {
        super::xdg_dirs(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| std::ffi::OsString::from(value))
        })
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_user_folders_follow_xdg() {
        let dirs = xdg(&[("HOME", "/home/me")]);
        assert_eq!(dirs.config, PathBuf::from("/home/me/.config/deskmate"));
        assert_eq!(dirs.data, PathBuf::from("/home/me/.local/share/deskmate"));

        let dirs = xdg(&[
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/data"),
        ]);
        assert_eq!(dirs.config, PathBuf::from("/cfg/deskmate"));
        assert_eq!(dirs.data, PathBuf::from("/data/deskmate"));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_relative_xdg_value_is_ignored() {
        let dirs = xdg(&[("HOME", "/home/me"), ("XDG_CONFIG_HOME", "cfg")]);
        assert_eq!(dirs.config, PathBuf::from("/home/me/.config/deskmate"));
        // Without HOME the folder is relative, as on Windows without APPDATA.
        assert_eq!(xdg(&[]).config, PathBuf::from(".").join("deskmate"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_daemon_folders_are_the_system_ones() {
        let dirs = super::Dirs::system();
        assert_eq!(dirs.config, PathBuf::from("/etc/deskmate"));
        assert_eq!(dirs.data, PathBuf::from("/var/lib/deskmate"));
    }

    #[cfg(not(windows))]
    #[test]
    fn config_file_is_in_the_configuration_folder() {
        assert!(super::config_path().ends_with("deskmate/config.json"));
    }

    /// GitHub issue #11: "Start minimized to tray" is stored the moment it is
    /// toggled, and the stored value is what the next start reads.
    #[test]
    fn start_minimized_is_stored_as_a_feature_flag() {
        let mut cfg = AppConfig::default();
        assert!(!cfg.launch_hidden);

        cfg.set_feature_flag("launch_hidden", true).unwrap();
        let raw = serde_json::to_string(&cfg).unwrap();
        let mut stored: AppConfig = serde_json::from_str(&raw).unwrap();
        assert!(stored.launch_hidden);

        stored.set_feature_flag("launch_hidden", false).unwrap();
        let raw = serde_json::to_string(&stored).unwrap();
        let stored: AppConfig = serde_json::from_str(&raw).unwrap();
        assert!(!stored.launch_hidden);
    }

    /// A config written before automatic updates: the daily check keeps its
    /// stored value, and the new mode starts at "ask".
    #[test]
    fn config_without_update_mode_asks_first() {
        let mut old = serde_json::to_value(AppConfig::default()).unwrap();
        let fields = old.as_object_mut().unwrap();
        fields.remove("update_mode");
        fields.remove("update_state");
        fields.insert("update_check".into(), false.into());
        fields.insert("update_notified".into(), "0.8.4".into());

        let cfg: AppConfig = serde_json::from_value(old).unwrap();
        assert_eq!(cfg.update_mode, "ask");
        assert!(!cfg.update_check);
        assert_eq!(cfg.update_notified, "0.8.4");
        assert_eq!(cfg.update_state.later_until, 0);
        assert!(cfg.update_state.installing.is_none());
        assert!(cfg.update_state.failed_version.is_empty());
    }

    #[test]
    fn update_mode_and_state_survive_a_save() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.update_mode, "ask");
        cfg.update_mode = "auto".into();
        cfg.update_state.later_until = 1_800_000_000;
        cfg.update_state.installing = Some(super::UpdateAttempt {
            from: "0.8.5".into(),
            to: "0.9.0".into(),
            auto: true,
            hidden: true,
            started_at: 1_799_999_000,
        });

        let raw = serde_json::to_string(&cfg).unwrap();
        let stored: AppConfig = serde_json::from_str(&raw).unwrap();
        assert_eq!(stored.update_mode, "auto");
        assert_eq!(stored.update_state, cfg.update_state);
    }

    #[test]
    fn unknown_feature_flag_is_refused() {
        let mut cfg = AppConfig::default();
        assert!(cfg.set_feature_flag("launch_hiden", true).is_err());
        assert!(!cfg.launch_hidden);
    }
}
