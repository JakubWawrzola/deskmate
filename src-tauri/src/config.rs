//! Konfiguracja aplikacji: %APPDATA%\Deskmate\config.json.
//! Haslo MQTT NIGDY nie laduje w JSON - trzymane w Windows Credential Manager.

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomCommand {
    /// identyfikator [a-z0-9_], czesc object_id encji w HA
    pub id: String,
    /// nazwa widoczna w HA
    pub name: String,
    /// program + argumenty wykonywane przez PowerShell (NIE z payloadu MQTT!)
    /// Wartosc kontrolki (switch on/off, number) trafia do skryptu jako
    /// zmienna srodowiskowa $env:DESKMATE_VALUE (bez interpolacji do kodu - anty-RCE).
    pub command: String,
    /// typ kontrolki w HA: button | switch | number
    #[serde(default = "default_kind")]
    pub kind: String,
    /// zakres dla number
    #[serde(default)]
    pub num_min: f64,
    #[serde(default = "default_num_max")]
    pub num_max: f64,
    #[serde(default = "default_num_step")]
    pub num_step: f64,
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub configured: bool,
    pub broker_host: String,
    pub broker_port: u16,
    pub username: String,
    /// przyjazna nazwa urzadzenia w HA (default: hostname)
    pub device_name: String,
    /// id w topicach/unique_id: [a-z0-9_], default: hostname zsanityzowany
    pub node_id: String,
    pub publish_interval_secs: u64,
    /// sensory wylaczalne; klucz = id sensora, brak wpisu = default danego sensora
    pub sensors_enabled: std::collections::HashMap<String, bool>,
    pub custom_commands: Vec<CustomCommand>,
    pub launch_hidden: bool,
    /// opt-in: pozwol HA wpisywac tekst / sterowac prezentacja (SendInput klawiszy)
    pub allow_input: bool,
    /// opt-in: pozwol HA kazac komputerowi mowic (TTS)
    pub tts_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let host = hostname();
        Self {
            configured: false,
            broker_host: String::new(),
            broker_port: 1883,
            username: String::new(),
            device_name: host.clone(),
            node_id: sanitize_id(&host),
            publish_interval_secs: 15,
            sensors_enabled: Default::default(),
            custom_commands: Vec::new(),
            launch_hidden: false,
            allow_input: false,
            tts_enabled: false,
        }
    }
}

pub fn hostname() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "windows-pc".into())
}

/// hostname -> bezpieczny id topicu/unique_id
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
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    // zapis atomowy: tmp + rename
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

/// Usuwa wpis w Credential Manager dla starego user@host (przy zmianie brokera),
/// zeby nie zostawal osierocony.
pub fn delete_password_for(username: &str, broker_host: &str) {
    let account = format!("{}@{}", username, broker_host);
    if let Ok(e) = keyring::Entry::new(consts::KEYRING_SERVICE, &account) {
        let _ = e.delete_credential();
    }
}
