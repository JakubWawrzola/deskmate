//! The pairing code that Home Assistant shows: `DMP1.` and base64url of a
//! JSON object with the key (`k`), the local (`u`) and remote (`r`) Link
//! addresses and, for a computer that is already paired, its node id (`n`)
//! and the cascade key (`c`).
//!
//! It is read exactly as the app reads it (`src/pairing.ts`), so a code that
//! works in the app works here, and one the app refuses is refused here. The
//! code holds the pairing key: nothing here logs it or puts it in an error.

use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::config::{ServiceConfig, DEFAULT_INTERVAL_SECS};

/// What every pairing code starts with.
pub const PREFIX: &str = "DMP1.";
const MAX_NODE_CHARS: usize = 64;

/// What a pairing code carries.
pub struct PairingCode {
    pub key: Zeroizing<String>,
    pub url: Option<String>,
    pub url_remote: Option<String>,
    pub node: Option<String>,
    pub cascade_key: Option<Zeroizing<String>>,
}

/// The browser's `atob`: standard base64 where whitespace is skipped, one or
/// two `=` at the end are optional and bits after the last byte are ignored.
fn forgiving_base64(text: &str) -> Option<Zeroizing<Vec<u8>>> {
    let mut data: Zeroizing<String> = Zeroizing::new(
        text.chars()
            .filter(|c| !matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' '))
            .collect(),
    );
    if data.len().is_multiple_of(4) {
        for _ in 0..2 {
            if data.ends_with('=') {
                data.pop();
            }
        }
    }
    if data.len() % 4 == 1 {
        return None;
    }
    let engine = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    );
    engine.decode(data.as_bytes()).ok().map(Zeroizing::new)
}

fn is_node_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_NODE_CHARS
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Reads a pasted pairing code. None when it is not one.
pub fn parse(input: &str) -> Option<PairingCode> {
    let text = input.trim();
    let body = text.strip_prefix(PREFIX)?;
    let mut body: Zeroizing<String> = Zeroizing::new(body.replace('-', "+").replace('_', "/"));
    while !body.len().is_multiple_of(4) {
        body.push('=');
    }
    let raw = forgiving_base64(&body)?;
    let data: Value = serde_json::from_slice(&raw).ok()?;
    let record = data.as_object()?;
    let text_field = |name: &str| record.get(name).and_then(Value::as_str);
    let key = Zeroizing::new(text_field("k")?.to_string());
    let url = text_field("u").map(str::to_string);
    let url_remote = text_field("r").map(str::to_string);
    let node = text_field("n")
        .filter(|node| is_node_id(node))
        .map(str::to_string);
    let cascade_key = text_field("c")
        .filter(|key| !key.is_empty())
        .map(|key| Zeroizing::new(key.to_string()));
    // Only a remote address known: it becomes the primary one.
    let (url, url_remote) = match url {
        Some(url) if !url.is_empty() => (Some(url), url_remote),
        _ => (url_remote, None),
    };
    Some(PairingCode {
        key,
        url,
        url_remote,
        node,
        cascade_key,
    })
}

impl PairingCode {
    /// The settings this code leads to. The node id comes from the code, or
    /// else is `default_node` (the one the app would pick on this computer).
    /// What the code does not carry, the interval, the device name and the
    /// sensors, is kept from `previous`.
    pub fn settings(
        &self,
        previous: Option<&ServiceConfig>,
        default_node: &str,
    ) -> Result<ServiceConfig, String> {
        let Some(url) = self.url.as_ref().filter(|url| !url.is_empty()) else {
            return Err("the pairing code has no address of Home Assistant. Set the address under Settings, System, Network in Home Assistant and show the code again".into());
        };
        ServiceConfig {
            node_id: self
                .node
                .clone()
                .unwrap_or_else(|| default_node.to_string()),
            link_url: url.clone(),
            link_url_remote: self.url_remote.clone().unwrap_or_default(),
            link_cascade: self.cascade_key.is_some(),
            publish_interval_secs: previous
                .map_or(DEFAULT_INTERVAL_SECS, |config| config.publish_interval_secs),
            device_name: previous
                .map(|config| config.device_name.clone())
                .unwrap_or_default(),
            sensors_enabled: previous
                .map(|config| config.sensors_enabled.clone())
                .unwrap_or_default(),
        }
        .validated()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    use serde_json::json;

    // base64 of 32 bytes of 0x01 and of 0x02
    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    /// A code as the integration makes it: base64url without padding.
    fn code(data: &Value) -> String {
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(data.to_string()))
    }

    #[test]
    fn reads_every_field_of_a_code() {
        let parsed = parse(&code(&json!({
            "k": KEY_A,
            "u": "ws://homeassistant.local:8123",
            "r": "wss://example.com",
            "n": "office_pc",
            "c": KEY_B,
        })))
        .unwrap();
        assert_eq!(parsed.key.as_str(), KEY_A);
        assert_eq!(parsed.url.as_deref(), Some("ws://homeassistant.local:8123"));
        assert_eq!(parsed.url_remote.as_deref(), Some("wss://example.com"));
        assert_eq!(parsed.node.as_deref(), Some("office_pc"));
        assert_eq!(
            parsed.cascade_key.as_deref().map(String::as_str),
            Some(KEY_B)
        );
    }

    #[test]
    fn only_the_key_is_required() {
        let parsed = parse(&code(&json!({"k": KEY_A}))).unwrap();
        assert_eq!(parsed.key.as_str(), KEY_A);
        assert!(parsed.url.is_none() && parsed.url_remote.is_none());
        assert!(parsed.node.is_none() && parsed.cascade_key.is_none());
    }

    #[test]
    fn a_remote_address_alone_becomes_the_primary_one() {
        for local in [None, Some(json!("")), Some(json!(5))] {
            let mut data = json!({"k": KEY_A, "r": "wss://example.com"});
            if let Some(local) = local {
                data["u"] = local;
            }
            let parsed = parse(&code(&data)).unwrap();
            assert_eq!(parsed.url.as_deref(), Some("wss://example.com"));
            assert_eq!(parsed.url_remote, None);
        }
    }

    #[test]
    fn leaves_out_a_node_or_cascade_key_that_cannot_be_used() {
        for node in [
            json!("Office-PC"),
            json!(""),
            json!("a".repeat(65)),
            json!(7),
        ] {
            let parsed = parse(&code(&json!({"k": KEY_A, "n": node}))).unwrap();
            assert!(parsed.node.is_none(), "{node}");
        }
        for cascade in [json!(""), json!(false), json!(null)] {
            let parsed = parse(&code(&json!({"k": KEY_A, "c": cascade}))).unwrap();
            assert!(parsed.cascade_key.is_none(), "{cascade}");
        }
        let parsed = parse(&code(&json!({"k": KEY_A, "u": 1, "r": ["x"]}))).unwrap();
        assert!(parsed.url.is_none() && parsed.url_remote.is_none());
    }

    #[test]
    fn tolerates_what_a_paste_adds_and_what_atob_forgives() {
        let data = json!({"k": KEY_A, "u": "ws://192.168.1.10:8123"});
        let plain = code(&data);
        assert!(parse(&format!("  {plain}\r\n")).is_some());
        // Padded, and in the standard alphabet instead of the URL one.
        let padded = format!("{PREFIX}{}", STANDARD.encode(data.to_string()));
        assert_eq!(
            parse(&padded).unwrap().url.as_deref(),
            Some("ws://192.168.1.10:8123")
        );
    }

    #[test]
    fn refuses_what_is_not_a_pairing_code() {
        let valid_body = URL_SAFE_NO_PAD.encode(json!({"k": KEY_A}).to_string());
        let inputs = [
            String::new(),
            KEY_A.to_string(),
            format!("dmp1.{valid_body}"),
            format!("DMP2.{valid_body}"),
            format!("DMP1{valid_body}"),
            "DMP1.".to_string(),
            "DMP1.!!!!".to_string(),
            "DMP1.A".to_string(),
            "DMP1.QQ====".to_string(),
            code(&json!(null)),
            code(&json!(7)),
            code(&json!("text")),
            code(&json!([KEY_A])),
            code(&json!({})),
            code(&json!({"k": 5})),
            code(&json!({"k": null, "u": "ws://192.168.1.10:8123"})),
            format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode("{\"k\":")),
            format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode("not json")),
        ];
        for input in inputs {
            assert!(parse(&input).is_none(), "{input}");
        }
    }

    #[test]
    fn turns_a_code_into_settings() {
        let parsed = parse(&code(&json!({
            "k": KEY_A,
            "u": "ws://homeassistant.local:8123",
            "r": "wss://example.com",
        })))
        .unwrap();
        let config = parsed.settings(None, "my_pc").unwrap();
        assert_eq!(config.node_id, "my_pc");
        assert_eq!(
            config.link_url,
            "ws://homeassistant.local:8123/api/deskmate_link/ws"
        );
        assert_eq!(
            config.link_url_remote,
            "wss://example.com/api/deskmate_link/ws"
        );
        assert!(!config.link_cascade);
        assert_eq!(config.publish_interval_secs, DEFAULT_INTERVAL_SECS);
        assert_eq!(config.device_name, "");

        let with_node = parse(&code(&json!({
            "k": KEY_A,
            "u": "ws://192.168.1.10:8123",
            "n": "office_pc",
            "c": KEY_B,
        })))
        .unwrap();
        let config = with_node.settings(None, "my_pc").unwrap();
        assert_eq!(config.node_id, "office_pc");
        assert!(config.link_cascade);
    }

    #[test]
    fn a_new_code_keeps_what_it_does_not_carry() {
        let mut previous = parse(&code(&json!({"k": KEY_A, "u": "ws://192.168.1.10:8123"})))
            .unwrap()
            .settings(None, "my_pc")
            .unwrap();
        previous.publish_interval_secs = 60;
        previous.device_name = "Office".into();
        previous.sensors_enabled.insert("cpu".into(), false);
        previous.node_id = "old_name".into();

        let config = parse(&code(&json!({"k": KEY_B, "u": "ws://192.168.1.11:8123"})))
            .unwrap()
            .settings(Some(&previous), "my_pc")
            .unwrap();
        assert_eq!(config.node_id, "my_pc", "the node id comes from the code");
        assert_eq!(
            config.link_url,
            "ws://192.168.1.11:8123/api/deskmate_link/ws"
        );
        assert_eq!(config.publish_interval_secs, 60);
        assert_eq!(config.device_name, "Office");
        assert_eq!(config.sensors_enabled.get("cpu"), Some(&false));
    }

    #[test]
    fn refuses_settings_without_a_usable_address() {
        let no_address = parse(&code(&json!({"k": KEY_A}))).unwrap();
        let error = no_address.settings(None, "my_pc").unwrap_err();
        assert!(error.contains("no address"), "{error}");

        let public_plain = parse(&code(&json!({"k": KEY_A, "u": "ws://example.com"}))).unwrap();
        let error = public_plain.settings(None, "my_pc").unwrap_err();
        assert!(error.starts_with("link_url:"), "{error}");
        assert!(!error.contains(KEY_A));
    }
}
