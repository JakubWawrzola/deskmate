//! What the app gives the Deskmate service: its settings, taken from the
//! app's own, and the pairing key and cascade key from Credential Manager.
//!
//! The settings and the fingerprint come from `deskmate_service::config`
//! and the line from `deskmate_service::admin`, the code the service reads
//! them with, so both sides always agree on the form.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use deskmate_core::link::{validate_pairing_key, SecretKey};
use deskmate_service::admin;
use deskmate_service::config::ServiceConfig;
use std::collections::BTreeMap;
use zeroize::Zeroizing;

use crate::config::AppConfig;

/// The keys the service needs, decoded and wiped when dropped.
pub struct ServiceKeys {
    pub pairing: SecretKey,
    /// Present exactly when cascade encryption is on.
    pub cascade: Option<SecretKey>,
}

/// A sensor setting the service accepts: the form of a node id.
fn is_sensor_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The service's settings for this computer, checked with the service's own
/// rules. The message says what the user can change.
pub fn service_config(cfg: &AppConfig) -> Result<ServiceConfig, String> {
    let sensors_enabled: BTreeMap<String, bool> = cfg
        .sensors_enabled
        .iter()
        .filter(|(key, _)| is_sensor_key(key))
        .map(|(key, enabled)| (key.clone(), *enabled))
        .collect();
    ServiceConfig {
        node_id: cfg.node_id.clone(),
        link_url: cfg.link_url.clone(),
        link_url_remote: cfg.link_url_remote.clone(),
        link_cascade: cfg.link_cascade,
        publish_interval_secs: cfg.publish_interval_secs,
        device_name: cfg.device_name.clone(),
        sensors_enabled,
    }
    .validated()
    .map_err(|error| format!("The service cannot use these settings: {error}."))
}

/// Decodes the keys as Credential Manager holds them. `cascade` is read only
/// when cascade encryption is on.
pub fn decode_keys(
    link_cascade: bool,
    pairing: &str,
    cascade: Option<&str>,
) -> Result<ServiceKeys, String> {
    let pairing = validate_pairing_key(pairing)
        .map_err(|_| "The pairing key of this computer cannot be read. Pair it again.")?;
    let cascade = if link_cascade {
        let raw = cascade.ok_or("Cascade encryption is on but its key is missing.")?;
        let key = validate_pairing_key(raw)
            .map_err(|_| "The cascade key cannot be read. Paste it again in Geeky stuff.")?;
        if *key == *pairing {
            return Err("The cascade key is the same as the pairing key.".into());
        }
        Some(key)
    } else {
        None
    };
    Ok(ServiceKeys { pairing, cascade })
}

/// Reads the keys of `cfg`'s node from Credential Manager.
pub fn load_keys(cfg: &AppConfig) -> Result<ServiceKeys, String> {
    let pairing = crate::config::get_link_key(&cfg.node_id)
        .map(Zeroizing::new)
        .ok_or("This computer is not paired.")?;
    let cascade = if cfg.link_cascade {
        crate::config::get_link_cascade_key(&cfg.node_id).map(Zeroizing::new)
    } else {
        None
    };
    decode_keys(
        cfg.link_cascade,
        &pairing,
        cascade.as_ref().map(|key| key.as_str()),
    )
}

/// The fingerprint the service shows when it holds exactly these settings
/// and keys.
pub fn fingerprint(config: &ServiceConfig, keys: &ServiceKeys) -> Result<String, String> {
    config.fingerprint(&keys.pairing, keys.cascade.as_deref())
}

/// The `provision` line for the one-time pipe, wiped when dropped. The keys
/// are encoded again from their bytes, so stray whitespace in the stored
/// text never reaches the service.
pub fn provision_line(
    config: &ServiceConfig,
    keys: &ServiceKeys,
) -> Result<Zeroizing<String>, String> {
    // By reference: a copy of the bytes on the stack would not be wiped.
    let pairing = Zeroizing::new(B64.encode(keys.pairing.as_slice()));
    let cascade = keys
        .cascade
        .as_ref()
        .map(|key| Zeroizing::new(B64.encode(key.as_slice())));
    admin::provision_frame(config, &pairing, cascade.as_ref().map(|key| key.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskmate_service::admin::Request;

    // base64 of 32 bytes of 0x01 and of 0x02, used nowhere else
    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    fn app_config() -> AppConfig {
        let mut cfg = AppConfig {
            transport: "link".into(),
            configured: true,
            node_id: "office_pc".into(),
            link_url: "ws://homeassistant.local:8123/api/deskmate_link/ws".into(),
            link_url_remote: "wss://example.com/api/deskmate_link/ws".into(),
            device_name: "Office PC".into(),
            publish_interval_secs: 20,
            ..AppConfig::default()
        };
        cfg.sensors_enabled.insert("cpu".into(), false);
        cfg.sensors_enabled.insert("disk_c_free".into(), true);
        cfg
    }

    fn parse(line: &str) -> Box<admin::Provision> {
        let line = line
            .strip_suffix('\n')
            .expect("one line with its line break");
        assert!(!line.contains('\n'));
        match admin::parse(line.as_bytes()) {
            Ok(Request::Provision(provision)) => provision,
            Ok(other) => panic!("parsed as {}", other.name()),
            Err(refusal) => panic!("refused: {}", refusal.detail()),
        }
    }

    #[test]
    fn the_line_is_what_the_service_accepts() {
        let cfg = app_config();
        let config = service_config(&cfg).unwrap();
        let keys = decode_keys(false, KEY_A, None).unwrap();
        let line = provision_line(&config, &keys).unwrap();
        let read = parse(&line);
        assert_eq!(read.config, config);
        assert_eq!(read.config.node_id, "office_pc");
        assert_eq!(read.config.link_url, cfg.link_url);
        assert_eq!(read.config.link_url_remote, cfg.link_url_remote);
        assert_eq!(read.config.publish_interval_secs, 20);
        assert_eq!(read.config.device_name, "Office PC");
        assert_eq!(read.config.sensors_enabled.get("cpu"), Some(&false));
        assert_eq!(read.config.sensors_enabled.get("disk_c_free"), Some(&true));
        assert_eq!(read.key.as_str(), KEY_A);
        assert!(read.cascade_key.is_none());
    }

    #[test]
    fn the_line_carries_the_cascade_key_only_with_cascade() {
        let mut cfg = app_config();
        cfg.link_cascade = true;
        let config = service_config(&cfg).unwrap();
        let keys = decode_keys(true, KEY_A, Some(KEY_B)).unwrap();
        let read = parse(&provision_line(&config, &keys).unwrap());
        assert!(read.config.link_cascade);
        assert_eq!(
            read.cascade_key.map(|k| k.as_str().to_string()),
            Some(KEY_B.into())
        );

        // A stored cascade key is left out while cascade is off.
        let keys = decode_keys(false, KEY_A, Some(KEY_B)).unwrap();
        assert!(keys.cascade.is_none());
        assert!(decode_keys(true, KEY_A, None).is_err());
        assert!(decode_keys(true, KEY_A, Some(KEY_A)).is_err());
    }

    #[test]
    fn the_fingerprint_matches_the_one_the_service_computes() {
        let cfg = app_config();
        let config = service_config(&cfg).unwrap();
        let keys = decode_keys(false, KEY_A, None).unwrap();
        let read = parse(&provision_line(&config, &keys).unwrap());
        // What the service computes from what it received and stored.
        let service_side = read.config.fingerprint(&[1u8; 32], None).unwrap();
        assert_eq!(fingerprint(&config, &keys).unwrap(), service_side);

        let mut other = cfg.clone();
        other.publish_interval_secs = 21;
        let other = service_config(&other).unwrap();
        assert_ne!(fingerprint(&other, &keys).unwrap(), service_side);
        let cascaded = decode_keys(true, KEY_A, Some(KEY_B)).unwrap();
        assert_ne!(fingerprint(&config, &cascaded).unwrap(), service_side);
    }

    #[test]
    fn stray_whitespace_in_a_stored_key_does_not_reach_the_service() {
        let config = service_config(&app_config()).unwrap();
        let keys = decode_keys(false, &format!(" {KEY_A}\n"), None).unwrap();
        assert_eq!(
            parse(&provision_line(&config, &keys).unwrap()).key.as_str(),
            KEY_A
        );
    }

    #[test]
    fn settings_the_service_refuses_are_reported_without_their_value() {
        let mut cfg = app_config();
        cfg.link_url = "ws://secret-host.example.com".into();
        let error = service_config(&cfg).unwrap_err();
        assert!(error.contains("link_url"), "{error}");
        assert!(!error.contains("secret-host"), "{error}");

        let mut cfg = app_config();
        cfg.device_name = "n".repeat(101);
        assert!(service_config(&cfg).is_err());

        assert!(decode_keys(false, "not base64", None).is_err());
        let error = decode_keys(false, "AQEB", None).err().unwrap_or_default();
        assert!(!error.contains("AQEB"), "{error}");
    }

    #[test]
    fn sensor_settings_that_are_no_key_are_left_out() {
        let mut cfg = app_config();
        cfg.sensors_enabled.insert("Not A Key".into(), true);
        let config = service_config(&cfg).unwrap();
        assert!(!config.sensors_enabled.contains_key("Not A Key"));
        assert_eq!(config.sensors_enabled.len(), 2);
    }
}
