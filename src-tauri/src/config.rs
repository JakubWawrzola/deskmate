//! Application configuration: %APPDATA%\Deskmate\config.json.
//! MQTT password NEVER lands in JSON - stored in Windows Credential Manager.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::consts;

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
fn default_transport() -> String {
    "mqtt".into()
}
fn default_clipboard_mode() -> String {
    "off".into()
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
    #[serde(default = "default_transport")]
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
    /// friendly device name in HA (default: hostname)
    pub device_name: String,
    /// id used in topics/unique_id: [a-z0-9_], default: sanitized hostname
    pub node_id: String,
    pub publish_interval_secs: u64,
    /// sensors that can be disabled; key = sensor id, missing entry = that sensor's default
    pub sensors_enabled: std::collections::HashMap<String, bool>,
    pub custom_commands: Vec<CustomCommand>,
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
    /// brand toasts as "HomeOS" (Start Menu shortcut with AUMID). When false
    /// or when branding fails -> toast via PowerShell AUMID (always works).
    #[serde(default = "default_true")]
    pub toast_branding: bool,
    /// Home Assistant URL (local), e.g. http://192.168.18.9:8123. Empty = API channel disabled.
    pub ha_url: String,
    /// fallback URL (Tailscale/public) - failover like the MQTT broker
    pub ha_url_remote: String,
    /// global keyboard shortcuts (work in the background)
    pub hotkeys: Vec<Hotkey>,
    /// widget panel tiles
    pub widgets: Vec<WidgetItem>,
    /// quick actions in the tray menu
    pub tray_actions: Vec<TrayAction>,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        let host = hostname();
        Self {
            configured: false,
            transport: default_transport(),
            broker_host: String::new(),
            broker_host_remote: String::new(),
            broker_port: 8883,
            mqtt_transport: default_mqtt_transport(),
            mqtt_ca_path: String::new(),
            username: String::new(),
            link_url: String::new(),
            link_url_remote: String::new(),
            device_name: host.clone(),
            node_id: sanitize_id(&host),
            publish_interval_secs: 15,
            sensors_enabled: Default::default(),
            custom_commands: Vec::new(),
            launch_hidden: false,
            allow_input: false,
            tts_enabled: false,
            clipboard_read_mode: default_clipboard_mode(),
            clipboard_write_mode: default_clipboard_mode(),
            allowed_url_origins: Vec::new(),
            toast_branding: true,
            ha_url: String::new(),
            ha_url_remote: String::new(),
            hotkeys: Vec::new(),
            widgets: Vec::new(),
            tray_actions: Vec::new(),
        }
    }
}

pub fn hostname() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "windows-pc".into())
}

/// hostname -> safe topic/unique_id id
pub fn sanitize_id(s: &str) -> String {
    let out: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() { "windows_pc".into() } else { trimmed.into() }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join(consts::CONFIG_DIR).join("config.json")
}

pub fn load() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let legacy_without_transport = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| value.as_object().map(|object| !object.contains_key("mqtt_transport")))
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

fn keyring_entry(cfg: &AppConfig) -> Result<keyring::Entry, String> {
    let account = format!("{}@{}", cfg.username, cfg.broker_host);
    keyring::Entry::new(consts::KEYRING_SERVICE, &account).map_err(|e| e.to_string())
}

pub fn set_password(cfg: &AppConfig, password: &str) -> Result<(), String> {
    let entry = keyring_entry(cfg)?;
    if password.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(password).map_err(|e| e.to_string())
}

pub fn get_password(cfg: &AppConfig) -> Option<String> {
    keyring_entry(cfg).ok()?.get_password().ok()
}

/// Removes the Credential Manager entry for the old user@host (when the broker
/// changes), so it doesn't stay orphaned.
pub fn delete_password_for(username: &str, broker_host: &str) {
    let account = format!("{}@{}", username, broker_host);
    if let Ok(e) = keyring::Entry::new(consts::KEYRING_SERVICE, &account) {
        let _ = e.delete_credential();
    }
}

fn link_keyring_entry(node_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(consts::LINK_KEYRING_SERVICE, node_id).map_err(|e| e.to_string())
}

pub fn set_link_key(node_id: &str, pairing_key: &str) -> Result<(), String> {
    let entry = link_keyring_entry(node_id)?;
    if pairing_key.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(pairing_key).map_err(|e| e.to_string())
}

pub fn get_link_key(node_id: &str) -> Option<String> {
    link_keyring_entry(node_id).ok()?.get_password().ok()
}

pub fn delete_link_key_for(node_id: &str) {
    if let Ok(entry) = link_keyring_entry(node_id) {
        let _ = entry.delete_credential();
    }
}
