//! The service's settings: `config.json` in the data folder. It holds only
//! what a client without a signed-in user needs, and never a key.
//!
//! The file is written by the service itself when it is provisioned. It is
//! read strictly: an unknown field or a value the app would not accept makes
//! the whole file invalid.

use deskmate_core::config::AppConfig;
use deskmate_core::link;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use zeroize::Zeroizing;

/// Name of the settings file in the data folder.
pub const FILE_NAME: &str = "config.json";
/// A settings file larger than this is refused unread.
const MAX_FILE_BYTES: u64 = 64 * 1024;
const MAX_ID_CHARS: usize = 64;
const MAX_DEVICE_NAME_CHARS: usize = 100;
const MAX_SENSOR_ENTRIES: usize = 256;
const MIN_INTERVAL_SECS: u64 = 2;
const MAX_INTERVAL_SECS: u64 = 3600;
pub(crate) const DEFAULT_INTERVAL_SECS: u64 = 15;
/// First part of what the fingerprint covers; a change of the canonical form
/// gets a new label.
const FINGERPRINT_LABEL: &[u8] = b"deskmate service config v1";

fn default_interval() -> u64 {
    DEFAULT_INTERVAL_SECS
}

/// The settings of the service, as stored and as sent in a `provision` frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    /// The computer's id in Home Assistant: `a-z`, `0-9` and `_`, 1 to 64
    /// characters. The same one the app on this computer uses.
    pub node_id: String,
    /// Link address of Home Assistant, `ws://` or `wss://`.
    pub link_url: String,
    /// Optional second address, tried when the first one fails.
    #[serde(default)]
    pub link_url_remote: String,
    /// Cascade encryption; needs the cascade key next to the pairing key.
    #[serde(default)]
    pub link_cascade: bool,
    /// Seconds between two `state` frames, 2 to 3600.
    #[serde(default = "default_interval")]
    pub publish_interval_secs: u64,
    /// Name of the device in Home Assistant. Empty: the computer's host name.
    #[serde(default)]
    pub device_name: String,
    /// Sensors switched on or off by key; a missing key means the sensor's
    /// default.
    #[serde(default)]
    pub sensors_enabled: BTreeMap<String, bool>,
}

fn is_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_ID_CHARS
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl ServiceConfig {
    /// Checks every value with the rules the app applies and puts the
    /// addresses into their complete form. The messages name the field and
    /// never repeat a value.
    pub fn validated(mut self) -> Result<Self, String> {
        if !is_id(&self.node_id) {
            return Err("node_id must be 1 to 64 characters of a-z, 0-9 and _".into());
        }
        self.link_url =
            link::normalize_url(&self.link_url).map_err(|e| format!("link_url: {e}"))?;
        if self.link_url.is_empty() {
            return Err("link_url is required".into());
        }
        self.link_url_remote = link::normalize_url(&self.link_url_remote)
            .map_err(|e| format!("link_url_remote: {e}"))?;
        if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&self.publish_interval_secs) {
            return Err("publish_interval_secs must be between 2 and 3600".into());
        }
        self.device_name = self.device_name.trim().to_string();
        if self.device_name.chars().count() > MAX_DEVICE_NAME_CHARS
            || self.device_name.chars().any(char::is_control)
        {
            return Err(
                "device_name must be at most 100 characters without control characters".into(),
            );
        }
        if self.sensors_enabled.len() > MAX_SENSOR_ENTRIES
            || !self.sensors_enabled.keys().all(|key| is_id(key))
        {
            return Err("sensors_enabled must hold at most 256 keys of a-z, 0-9 and _".into());
        }
        Ok(self)
    }

    /// Reads the text of a settings file.
    pub fn parse(raw: &[u8]) -> Result<Self, String> {
        let config: Self = serde_json::from_slice(raw).map_err(|e| {
            // The category and the place, never the text that was read.
            format!(
                "not a valid settings object ({:?} at line {}, column {})",
                e.classify(),
                e.line(),
                e.column()
            )
        })?;
        config.validated()
    }

    /// Loads the settings from the data folder. Ok(None) when the file does
    /// not exist, which means the service was never provisioned.
    pub fn load(dir: &Path) -> Result<Option<Self>, String> {
        let path = dir.join(FILE_NAME);
        let size = match fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("cannot read the settings file: {error}")),
        };
        if size > MAX_FILE_BYTES {
            return Err("the settings file is too large".into());
        }
        let raw = fs::read(&path).map_err(|e| format!("cannot read the settings file: {e}"))?;
        Self::parse(&raw).map(Some)
    }

    /// Writes the settings to the data folder: a temporary file first, then a
    /// rename, so a reader never sees half a file.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let raw = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let path = dir.join(FILE_NAME);
        let temporary = dir.join(format!("{FILE_NAME}.tmp"));
        fs::write(&temporary, raw).map_err(|e| format!("cannot write the settings file: {e}"))?;
        fs::rename(&temporary, &path).map_err(|e| format!("cannot write the settings file: {e}"))
    }

    /// Removes the settings file. A file that is already gone is not an error.
    pub fn remove(dir: &Path) -> Result<(), String> {
        match fs::remove_file(dir.join(FILE_NAME)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("cannot remove the settings file: {error}")),
        }
    }

    /// The addresses to try, in order: the local one, then the remote one
    /// when it is set and different.
    pub fn endpoints(&self) -> Vec<String> {
        let mut endpoints = vec![self.link_url.clone()];
        if !self.link_url_remote.is_empty() && self.link_url_remote != self.link_url {
            endpoints.push(self.link_url_remote.clone());
        }
        endpoints
    }

    /// The same settings in the form the core functions read. Everything a
    /// service has no use for keeps the app's default.
    pub fn to_app_config(&self) -> AppConfig {
        let defaults = AppConfig::default();
        AppConfig {
            configured: true,
            transport: "link".into(),
            link_url: self.link_url.clone(),
            link_url_remote: self.link_url_remote.clone(),
            link_cascade: self.link_cascade,
            device_name: if self.device_name.is_empty() {
                defaults.device_name.clone()
            } else {
                self.device_name.clone()
            },
            node_id: self.node_id.clone(),
            publish_interval_secs: self.publish_interval_secs,
            sensors_enabled: self
                .sensors_enabled
                .iter()
                .map(|(key, enabled)| (key.clone(), *enabled))
                .collect(),
            ..defaults
        }
    }

    /// The bytes the fingerprint covers: every field of the validated
    /// settings in a fixed order, each with its length in front
    /// (`deskmate_core::link::enc`), then the cascade key or nothing.
    ///
    /// 1. `deskmate service config v1`
    /// 2. `node_id`
    /// 3. `link_url`, in its complete form
    /// 4. `link_url_remote`, in its complete form or empty
    /// 5. `1` or `0` for `link_cascade`
    /// 6. `publish_interval_secs` in decimal
    /// 7. `device_name`
    /// 8. the number of `sensors_enabled` entries in decimal
    /// 9. for every entry, sorted by key: the key, then `1` or `0`
    /// 10. the 32 bytes of the cascade key, or nothing without one
    ///
    /// The bytes hold the cascade key, so they are wiped when dropped. `enc`
    /// sizes its buffer up front and never leaves a copy behind by growing.
    fn canonical(&self, cascade: Option<&[u8; 32]>) -> Zeroizing<Vec<u8>> {
        let interval = self.publish_interval_secs.to_string();
        let count = self.sensors_enabled.len().to_string();
        let flag = |on: bool| -> &'static [u8] {
            if on {
                b"1"
            } else {
                b"0"
            }
        };
        let mut parts: Vec<&[u8]> = vec![
            FINGERPRINT_LABEL,
            self.node_id.as_bytes(),
            self.link_url.as_bytes(),
            self.link_url_remote.as_bytes(),
            flag(self.link_cascade),
            interval.as_bytes(),
            self.device_name.as_bytes(),
            count.as_bytes(),
        ];
        for (key, enabled) in &self.sensors_enabled {
            parts.push(key.as_bytes());
            parts.push(flag(*enabled));
        }
        parts.push(cascade.map_or(&[][..], |key| &key[..]));
        Zeroizing::new(link::enc(&parts))
    }

    /// Tells whether two sides hold the same settings and keys without
    /// showing either: HMAC-SHA256 under the pairing key over the canonical
    /// form, as 64 lowercase hex digits. Whoever holds the pairing key and
    /// the settings can compute it; nobody else learns anything from it.
    pub fn fingerprint(
        &self,
        pairing: &[u8; 32],
        cascade: Option<&[u8; 32]>,
    ) -> Result<String, String> {
        let mac = link::mac(pairing, &self.canonical(cascade))?;
        Ok(mac.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn valid() -> Value {
        json!({
            "node_id": "office_pc",
            "link_url": "ws://homeassistant.local:8123",
            "link_url_remote": "wss://example.com",
            "link_cascade": false,
            "publish_interval_secs": 30,
            "device_name": "Office PC",
            "sensors_enabled": {"cpu": false, "gpu_usage": true},
        })
    }

    fn parse(value: &Value) -> Result<ServiceConfig, String> {
        ServiceConfig::parse(value.to_string().as_bytes())
    }

    fn with(field: &str, value: Value) -> Value {
        let mut config = valid();
        config[field] = value;
        config
    }

    #[test]
    fn reads_a_complete_file_and_completes_the_addresses() {
        let config = parse(&valid()).unwrap();
        assert_eq!(config.node_id, "office_pc");
        assert_eq!(
            config.link_url,
            "ws://homeassistant.local:8123/api/deskmate_link/ws"
        );
        assert_eq!(
            config.link_url_remote,
            "wss://example.com/api/deskmate_link/ws"
        );
        assert_eq!(config.publish_interval_secs, 30);
        assert_eq!(config.sensors_enabled.get("cpu"), Some(&false));
        assert_eq!(
            config.endpoints(),
            [config.link_url.clone(), config.link_url_remote.clone()]
        );
    }

    #[test]
    fn only_the_node_and_the_address_are_required() {
        let config = parse(&json!({
            "node_id": "pc",
            "link_url": "ws://192.168.1.10:8123",
        }))
        .unwrap();
        assert_eq!(config.link_url_remote, "");
        assert!(!config.link_cascade);
        assert_eq!(config.publish_interval_secs, 15);
        assert_eq!(config.device_name, "");
        assert!(config.sensors_enabled.is_empty());
        assert_eq!(config.endpoints().len(), 1);

        for missing in ["node_id", "link_url"] {
            let mut config = valid();
            config.as_object_mut().unwrap().remove(missing);
            assert!(parse(&config).is_err(), "{missing}");
        }
    }

    #[test]
    fn refuses_what_does_not_belong_in_the_file() {
        // A key never belongs here, and neither does any other app setting.
        for (field, value) in [
            ("key", json!("AAAA")),
            ("cascade_key", json!("AAAA")),
            ("allow_input", json!(true)),
            ("custom_commands", json!([])),
            ("v", json!(1)),
        ] {
            assert!(parse(&with(field, value)).is_err(), "{field}");
        }
        for raw in [
            "",
            "[]",
            "null",
            "\"text\"",
            "{",
            "{\"node_id\": \"pc\"} trailing",
        ] {
            assert!(ServiceConfig::parse(raw.as_bytes()).is_err(), "{raw}");
        }
    }

    #[test]
    fn refuses_values_the_app_would_not_accept() {
        let long_id = "a".repeat(65);
        let long_name = "n".repeat(101);
        let too_many: serde_json::Map<String, Value> =
            (0..257).map(|n| (format!("s{n}"), json!(true))).collect();
        let cases = [
            ("node_id", json!("")),
            ("node_id", json!("Office-PC")),
            ("node_id", json!("office pc")),
            ("node_id", json!(long_id)),
            ("node_id", json!(7)),
            ("link_url", json!("")),
            ("link_url", json!("http://homeassistant.local:8123")),
            ("link_url", json!("ws://example.com")),
            ("link_url", json!("wss://user:secret@example.com")),
            ("link_url", json!("wss://example.com/other/path")),
            ("link_url_remote", json!("ws://8.8.8.8:8123")),
            ("link_url_remote", json!("not a url")),
            ("link_cascade", json!("yes")),
            ("publish_interval_secs", json!(1)),
            ("publish_interval_secs", json!(3601)),
            ("publish_interval_secs", json!(-5)),
            ("publish_interval_secs", json!("15")),
            ("device_name", json!(long_name)),
            ("device_name", json!("Office\nPC")),
            ("sensors_enabled", json!({"CPU": true})),
            ("sensors_enabled", json!({"cpu": "on"})),
            ("sensors_enabled", json!(["cpu"])),
            ("sensors_enabled", Value::Object(too_many)),
        ];
        for (field, value) in cases {
            assert!(
                parse(&with(field, value.clone())).is_err(),
                "{field}: {value}"
            );
        }
    }

    #[test]
    fn a_refusal_names_the_field_and_not_the_value() {
        let error = parse(&with("link_url", json!("ws://secret-host.example.com"))).unwrap_err();
        assert!(error.starts_with("link_url:"), "{error}");
        assert!(!error.contains("secret-host"), "{error}");
        let error = parse(&with("unexpected_secret_field", json!(1))).unwrap_err();
        assert!(!error.contains("unexpected_secret_field"), "{error}");
    }

    #[test]
    fn maps_onto_the_app_settings_the_core_reads() {
        let config = parse(&valid()).unwrap();
        let app = config.to_app_config();
        assert!(app.configured);
        assert_eq!(app.transport, "link");
        assert_eq!(app.node_id, "office_pc");
        assert_eq!(app.link_url, config.link_url);
        assert_eq!(app.link_url_remote, config.link_url_remote);
        assert_eq!(app.device_name, "Office PC");
        assert_eq!(app.publish_interval_secs, 30);
        assert_eq!(app.sensors_enabled.get("cpu"), Some(&false));
        assert_eq!(app.sensors_enabled.get("gpu_usage"), Some(&true));
        // Nothing that acts in a user's session is switched on.
        assert!(!app.allow_input);
        assert!(app.custom_commands.is_empty());
        assert!(app.link_file_roots.is_empty());

        let unnamed = parse(&with("device_name", json!(""))).unwrap();
        assert_eq!(
            unnamed.to_app_config().device_name,
            AppConfig::default().device_name
        );
    }

    #[test]
    fn written_settings_read_back_the_same() {
        let dir = std::env::temp_dir().join(format!(
            "deskmate-service-config-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(ServiceConfig::load(&dir), Ok(None));

        let config = parse(&valid()).unwrap();
        config.save(&dir).unwrap();
        assert_eq!(ServiceConfig::load(&dir), Ok(Some(config)));

        fs::write(dir.join(FILE_NAME), b"{\"node_id\": \"pc\"").unwrap();
        assert!(ServiceConfig::load(&dir).is_err());

        ServiceConfig::remove(&dir).unwrap();
        ServiceConfig::remove(&dir).unwrap();
        assert_eq!(ServiceConfig::load(&dir), Ok(None));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fingerprint_changes_with_every_setting_and_every_key() {
        let key = [7u8; 32];
        let cascade = [9u8; 32];
        let base = parse(&valid()).unwrap();
        let fingerprint = base.fingerprint(&key, None).unwrap();
        assert_eq!(fingerprint.len(), 64);
        assert!(fingerprint
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(fingerprint, base.fingerprint(&key, None).unwrap());

        let changed = [
            with("node_id", json!("other_pc")),
            with("link_url", json!("ws://192.168.1.10:8123")),
            with("link_url_remote", json!("")),
            with("link_cascade", json!(true)),
            with("publish_interval_secs", json!(31)),
            with("device_name", json!("Office")),
            with("sensors_enabled", json!({"cpu": true, "gpu_usage": true})),
            with("sensors_enabled", json!({"cpu": false})),
        ];
        for value in changed {
            let other = parse(&value).unwrap();
            assert_ne!(
                other.fingerprint(&key, None).unwrap(),
                fingerprint,
                "{value}"
            );
        }
        assert_ne!(base.fingerprint(&[8u8; 32], None).unwrap(), fingerprint);
        assert_ne!(base.fingerprint(&key, Some(&cascade)).unwrap(), fingerprint);
    }

    #[test]
    fn fingerprint_does_not_mix_up_neighbouring_fields() {
        let key = [7u8; 32];
        let first = parse(&json!({
            "node_id": "pc",
            "link_url": "ws://192.168.1.10:8123",
            "sensors_enabled": {"a": true, "b": false},
        }))
        .unwrap();
        let second = parse(&json!({
            "node_id": "pc",
            "link_url": "ws://192.168.1.10:8123",
            "sensors_enabled": {"a": false, "b": true},
        }))
        .unwrap();
        assert_ne!(
            first.fingerprint(&key, None).unwrap(),
            second.fingerprint(&key, None).unwrap()
        );
    }

    /// The app computes the same value on its side, so the form is fixed.
    #[test]
    fn fingerprint_has_a_fixed_form() {
        let config = parse(&json!({
            "node_id": "pc",
            "link_url": "ws://192.168.1.10:8123",
            "sensors_enabled": {"cpu": false},
        }))
        .unwrap();
        let expected = link::enc(&[
            b"deskmate service config v1",
            b"pc",
            b"ws://192.168.1.10:8123/api/deskmate_link/ws",
            b"",
            b"0",
            b"15",
            b"",
            b"1",
            b"cpu",
            b"0",
            b"",
        ]);
        assert_eq!(*config.canonical(None), expected);
        let mac = link::mac(&[7u8; 32], &expected).unwrap();
        let hex: String = mac.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(config.fingerprint(&[7u8; 32], None).unwrap(), hex);
    }

    #[test]
    fn the_canonical_form_with_a_cascade_key_is_wiped_when_dropped() {
        let config = parse(&json!({
            "node_id": "pc",
            "link_url": "ws://192.168.1.10:8123",
            "link_cascade": true,
        }))
        .unwrap();
        let cascade = [9u8; 32];
        let canonical: Zeroizing<Vec<u8>> = config.canonical(Some(&cascade));
        assert!(canonical.ends_with(&cascade));
        // One allocation of the exact size: no copy was freed while it grew.
        assert_eq!(canonical.capacity(), canonical.len());
    }
}
