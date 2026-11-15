//! The frames of the admin pipe: how an administrator's program sets the
//! service up, clears it and makes it read its settings again.
//!
//! One JSON object per line, at most 8192 bytes, always with the protocol
//! version:
//!
//! ```json
//! {"v":1,"op":"provision","node_id":"my_pc","link_url":"ws://homeassistant.local:8123","key":"<base64>"}
//! {"v":1,"op":"deprovision"}
//! {"v":1,"op":"reload"}
//! ```
//!
//! `provision` carries the fields of `ServiceConfig`, the pairing key as
//! `key` and, with `link_cascade`, the cascade key as `cascade_key`. Anything
//! else is refused: a longer line, text that is not one JSON object, another
//! version, an unknown operation, an unknown, missing or invalid field.
//!
//! Nothing that arrives here is logged, and what carried a key is wiped.

use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

use crate::config::{ServiceConfig, DEFAULT_INTERVAL_SECS};
use crate::secrets::Keys;

/// Version of the admin pipe's frames.
pub const PROTOCOL_VERSION: u64 = 1;
/// Longest accepted line, without its line break.
pub const MAX_FRAME_BYTES: usize = 8192;

/// A key in its text form, wiped when dropped, also when the frame around it
/// turns out to be invalid.
pub struct SecretText(Zeroizing<String>);

impl SecretText {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|text| Self(Zeroizing::new(text)))
    }
}

/// A checked `provision` frame.
pub struct Provision {
    pub config: ServiceConfig,
    pub key: SecretText,
    pub cascade_key: Option<SecretText>,
}

/// What an admin frame asks for.
pub enum Request {
    /// Store these settings and keys and connect with them.
    Provision(Box<Provision>),
    /// Forget settings and keys and disconnect.
    Deprovision,
    /// Read the settings and keys again.
    Reload,
}

impl Request {
    /// The operation's name, safe to log.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Provision(_) => "provision",
            Self::Deprovision => "deprovision",
            Self::Reload => "reload",
        }
    }
}

/// Why a frame was refused. The code goes into the answer; none of them
/// repeats what was received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    TooLong,
    NotJson,
    Version,
    UnknownOperation,
    /// A field is missing, unknown or has a value that is not accepted. The
    /// text names the field.
    Invalid(String),
}

impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLong => "too_long",
            Self::NotJson => "not_json",
            Self::Version => "version",
            Self::UnknownOperation => "unknown_operation",
            Self::Invalid(_) => "invalid",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::TooLong => "a frame is one line of at most 8192 bytes",
            Self::NotJson => "a frame is one JSON object",
            Self::Version => "this service speaks version 1 of the admin protocol",
            Self::UnknownOperation => "the operation is not one of provision, deprovision, reload",
            Self::Invalid(detail) => detail,
        }
    }
}

/// The two fields every frame has. Everything else is skipped here without
/// being copied, and read strictly once the operation is known.
#[derive(Deserialize)]
struct Head {
    v: Option<Value>,
    op: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bare {
    #[serde(rename = "v")]
    _v: u64,
    #[serde(rename = "op")]
    _op: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionFrame {
    #[serde(rename = "v")]
    _v: u64,
    #[serde(rename = "op")]
    _op: String,
    node_id: String,
    link_url: String,
    #[serde(default)]
    link_url_remote: String,
    #[serde(default)]
    link_cascade: bool,
    publish_interval_secs: Option<u64>,
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    sensors_enabled: BTreeMap<String, bool>,
    key: SecretText,
    cascade_key: Option<SecretText>,
}

fn invalid(error: serde_json::Error) -> Refusal {
    // serde names the field it stopped at. For a missing field that name is
    // ours; an unknown field's name came from the sender, so it is left out.
    let text = error.to_string();
    let detail = match text.strip_prefix("missing field `") {
        Some(rest) => {
            let field = rest.split('`').next().unwrap_or_default();
            format!("the field {field} is missing")
        }
        None if text.starts_with("unknown field") => {
            "the frame has a field that does not belong to this operation".to_string()
        }
        None => "a field has a value of the wrong type".to_string(),
    };
    Refusal::Invalid(detail)
}

/// Reads one line from the admin pipe, without its line break.
pub fn parse(line: &[u8]) -> Result<Request, Refusal> {
    if line.len() > MAX_FRAME_BYTES {
        return Err(Refusal::TooLong);
    }
    // serde would also read a struct from a JSON array; a frame is an object.
    if line.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(Refusal::NotJson);
    }
    let head: Head = serde_json::from_slice(line).map_err(|_| Refusal::NotJson)?;
    if head.v.as_ref().and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(Refusal::Version);
    }
    match head.op.as_ref().and_then(Value::as_str) {
        Some("provision") => {
            let frame: ProvisionFrame = serde_json::from_slice(line).map_err(invalid)?;
            let config = ServiceConfig {
                node_id: frame.node_id,
                link_url: frame.link_url,
                link_url_remote: frame.link_url_remote,
                link_cascade: frame.link_cascade,
                // The same default as a settings file without the field.
                publish_interval_secs: frame.publish_interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS),
                device_name: frame.device_name,
                sensors_enabled: frame.sensors_enabled,
            }
            .validated()
            .map_err(Refusal::Invalid)?;
            match (config.link_cascade, frame.cascade_key.is_some()) {
                (true, false) => {
                    return Err(Refusal::Invalid(
                        "link_cascade is on, so cascade_key is required".into(),
                    ))
                }
                (false, true) => {
                    return Err(Refusal::Invalid(
                        "cascade_key belongs to a frame with link_cascade on".into(),
                    ))
                }
                _ => {}
            }
            // Decoded once here so that a bad key is refused before anything
            // is stored; the decoded copy is wiped when it goes out of scope.
            Keys::decode(
                frame.key.as_str(),
                frame.cascade_key.as_ref().map(SecretText::as_str),
            )
            .map_err(|error| Refusal::Invalid(format!("key: {error}")))?;
            Ok(Request::Provision(Box::new(Provision {
                config,
                key: frame.key,
                cascade_key: frame.cascade_key,
            })))
        }
        Some("deprovision") => {
            serde_json::from_slice::<Bare>(line).map_err(invalid)?;
            Ok(Request::Deprovision)
        }
        Some("reload") => {
            serde_json::from_slice::<Bare>(line).map_err(invalid)?;
            Ok(Request::Reload)
        }
        _ => Err(Refusal::UnknownOperation),
    }
}

/// The answer to an accepted frame, as one line.
pub fn accepted() -> String {
    let mut line = json!({"v": PROTOCOL_VERSION, "ok": true}).to_string();
    line.push('\n');
    line
}

/// The answer to a frame that was refused or could not be carried out, as
/// one line. `detail` must not repeat what was received.
pub fn refused(code: &str, detail: &str) -> String {
    let mut line =
        json!({"v": PROTOCOL_VERSION, "ok": false, "error": code, "detail": detail}).to_string();
    line.push('\n');
    line
}

/// A `provision` frame as one line, for a program that sets the service up.
/// The line holds the keys and is wiped when dropped; no other copy of them
/// is made here. Err when a key is not base64 text.
pub fn provision_frame(
    config: &ServiceConfig,
    key: &str,
    cascade_key: Option<&str>,
) -> Result<Zeroizing<String>, String> {
    let is_base64 = |text: &str| {
        text.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    };
    if !is_base64(key) || !cascade_key.is_none_or(is_base64) {
        return Err("a key must be base64 text".into());
    }
    let settings = serde_json::to_string(config).map_err(|e| e.to_string())?;
    // The settings are one JSON object; its fields go into the frame as they
    // are. Base64 needs no escaping.
    let fields = settings
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| "the settings did not serialize as an object".to_string())?;
    let key_bytes = key.len() + cascade_key.map_or(0, str::len);
    // Sized up front: growing the text would leave an unwiped copy behind.
    let mut line = Zeroizing::new(String::with_capacity(fields.len() + key_bytes + 64));
    line.push_str(r#"{"v":1,"op":"provision","#);
    line.push_str(fields);
    line.push_str(r#","key":""#);
    line.push_str(key);
    line.push('"');
    if let Some(cascade_key) = cascade_key {
        line.push_str(r#","cascade_key":""#);
        line.push_str(cascade_key);
        line.push('"');
    }
    line.push_str("}\n");
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    // base64 of 32 bytes of 0x01 and of 0x02
    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    fn provision() -> Value {
        json!({
            "v": 1,
            "op": "provision",
            "node_id": "office_pc",
            "link_url": "ws://homeassistant.local:8123",
            "link_url_remote": "",
            "link_cascade": false,
            "publish_interval_secs": 20,
            "device_name": "Office PC",
            "sensors_enabled": {"cpu": false},
            "key": KEY_A,
        })
    }

    fn refusal(frame: &Value) -> Refusal {
        match parse(frame.to_string().as_bytes()) {
            Ok(request) => panic!("{} was accepted", request.name()),
            Err(refusal) => refusal,
        }
    }

    fn with(field: &str, value: Value) -> Value {
        let mut frame = provision();
        frame[field] = value;
        frame
    }

    fn without(field: &str) -> Value {
        let mut frame = provision();
        frame.as_object_mut().unwrap().remove(field);
        frame
    }

    #[test]
    fn reads_a_provision_frame() {
        let Ok(Request::Provision(request)) = parse(provision().to_string().as_bytes()) else {
            panic!("the frame must be accepted");
        };
        assert_eq!(request.config.node_id, "office_pc");
        assert_eq!(
            request.config.link_url,
            "ws://homeassistant.local:8123/api/deskmate_link/ws"
        );
        assert_eq!(request.config.publish_interval_secs, 20);
        assert_eq!(request.config.sensors_enabled.get("cpu"), Some(&false));
        assert_eq!(request.key.as_str(), KEY_A);
        assert!(request.cascade_key.is_none());
    }

    #[test]
    fn reads_a_provision_frame_with_cascade_and_with_defaults() {
        let mut frame = with("link_cascade", json!(true));
        frame["cascade_key"] = json!(KEY_B);
        let Ok(Request::Provision(request)) = parse(frame.to_string().as_bytes()) else {
            panic!("the frame must be accepted");
        };
        assert!(request.config.link_cascade);
        assert_eq!(
            request.cascade_key.map(|key| key.as_str().to_string()),
            Some(KEY_B.into())
        );

        let minimal = json!({
            "v": 1,
            "op": "provision",
            "node_id": "pc",
            "link_url": "ws://192.168.1.10:8123",
            "key": KEY_A,
        });
        let Ok(Request::Provision(request)) = parse(minimal.to_string().as_bytes()) else {
            panic!("the frame must be accepted");
        };
        assert_eq!(request.config.publish_interval_secs, 15);
        assert_eq!(request.config.device_name, "");
    }

    #[test]
    fn reads_the_operations_without_arguments() {
        assert!(matches!(
            parse(br#"{"v":1,"op":"deprovision"}"#),
            Ok(Request::Deprovision)
        ));
        assert!(matches!(
            parse(br#"{"op":"reload","v":1}"#),
            Ok(Request::Reload)
        ));
    }

    #[test]
    fn refuses_a_line_that_is_too_long() {
        let padding = "x".repeat(MAX_FRAME_BYTES);
        let frame = with("device_name", json!(padding));
        assert_eq!(refusal(&frame), Refusal::TooLong);

        // Exactly at the limit the line is read; its content is still checked.
        let mut line = br#"{"v":1,"op":"reload""#.to_vec();
        line.resize(MAX_FRAME_BYTES - 1, b' ');
        line.push(b'}');
        assert_eq!(line.len(), MAX_FRAME_BYTES);
        assert!(matches!(parse(&line), Ok(Request::Reload)));
        line.insert(1, b' ');
        assert!(matches!(parse(&line), Err(Refusal::TooLong)));
    }

    #[test]
    fn refuses_what_is_not_one_json_object() {
        for line in [
            &b""[..],
            b"provision",
            b"[1]",
            b"[1,\"reload\"]",
            b"\"reload\"",
            b"{\"v\":1,\"op\":\"reload\"",
            b"{\"v\":1,\"op\":\"reload\"}{\"v\":1,\"op\":\"reload\"}",
            b"\xff\xfe",
        ] {
            assert!(matches!(parse(line), Err(Refusal::NotJson)), "{line:?}");
        }
    }

    #[test]
    fn refuses_another_version() {
        for version in [
            json!(0),
            json!(2),
            json!("1"),
            json!(1.5),
            json!(null),
            json!(-1),
        ] {
            assert_eq!(
                refusal(&with("v", version.clone())),
                Refusal::Version,
                "{version}"
            );
        }
        assert_eq!(refusal(&without("v")), Refusal::Version);
        assert_eq!(refusal(&json!({"op": "reload"})), Refusal::Version);
    }

    #[test]
    fn refuses_an_unknown_operation() {
        for op in [
            json!("status"),
            json!("Provision"),
            json!(""),
            json!(7),
            json!(null),
        ] {
            assert_eq!(
                refusal(&with("op", op.clone())),
                Refusal::UnknownOperation,
                "{op}"
            );
        }
        assert_eq!(refusal(&json!({"v": 1})), Refusal::UnknownOperation);
    }

    #[test]
    fn refuses_missing_fields() {
        for field in ["node_id", "link_url", "key"] {
            let Refusal::Invalid(detail) = refusal(&without(field)) else {
                panic!("{field}: expected an invalid frame");
            };
            assert_eq!(detail, format!("the field {field} is missing"));
        }
    }

    #[test]
    fn refuses_unknown_and_mistyped_fields() {
        let unknown = refusal(&with("allow_input", json!(true)));
        let Refusal::Invalid(detail) = &unknown else {
            panic!("expected an invalid frame");
        };
        assert!(!detail.contains("allow_input"), "{detail}");

        for (field, value) in [
            ("node_id", json!(7)),
            ("link_cascade", json!("no")),
            ("publish_interval_secs", json!("20")),
            ("sensors_enabled", json!(["cpu"])),
            ("key", json!(["a"])),
        ] {
            assert!(
                matches!(refusal(&with(field, value)), Refusal::Invalid(_)),
                "{field}"
            );
        }
        // The operations without arguments take no extra field either.
        assert!(matches!(
            parse(br#"{"v":1,"op":"deprovision","node_id":"pc"}"#),
            Err(Refusal::Invalid(_))
        ));
        assert!(matches!(
            parse(br#"{"v":1,"op":"reload","force":true}"#),
            Err(Refusal::Invalid(_))
        ));
    }

    #[test]
    fn refuses_values_the_settings_would_not_accept() {
        for (field, value) in [
            ("node_id", json!("Office PC")),
            ("link_url", json!("ws://example.com")),
            ("link_url", json!("")),
            ("publish_interval_secs", json!(0)),
        ] {
            assert!(
                matches!(refusal(&with(field, value)), Refusal::Invalid(_)),
                "{field}"
            );
        }
    }

    #[test]
    fn refuses_keys_that_cannot_be_used() {
        for key in [json!(""), json!("AQEB"), json!("not base64")] {
            assert!(matches!(refusal(&with("key", key)), Refusal::Invalid(_)));
        }
        // Cascade and its key come together or not at all.
        assert!(matches!(
            refusal(&with("link_cascade", json!(true))),
            Refusal::Invalid(_)
        ));
        assert!(matches!(
            refusal(&with("cascade_key", json!(KEY_B))),
            Refusal::Invalid(_)
        ));
        let mut same = with("link_cascade", json!(true));
        same["cascade_key"] = json!(KEY_A);
        assert!(matches!(refusal(&same), Refusal::Invalid(_)));
    }

    #[test]
    fn a_refusal_never_repeats_what_was_received() {
        let marker = "repeat-me-and-the-test-fails";
        let frames = [
            with("key", json!(marker)),
            with("link_url", json!(format!("ws://{marker}.example.com"))),
            with(marker, json!(1)),
            with("op", json!(marker)),
            with("v", json!(marker)),
        ];
        for frame in frames {
            let refusal = refusal(&frame);
            assert!(!refusal.detail().contains(marker), "{}", refusal.detail());
            let answer = refused(refusal.code(), refusal.detail());
            assert!(!answer.contains(marker), "{answer}");
        }
    }

    #[test]
    fn answers_are_single_lines() {
        let answer = accepted();
        assert!(answer.ends_with('\n') && answer.matches('\n').count() == 1);
        let answer: Value = serde_json::from_str(&answer).unwrap();
        assert_eq!(answer, json!({"v": 1, "ok": true}));
        let answer: Value =
            serde_json::from_str(&refused("invalid", "the field key is missing")).unwrap();
        assert_eq!(
            answer,
            json!({"v": 1, "ok": false, "error": "invalid", "detail": "the field key is missing"})
        );
    }

    #[test]
    fn a_built_provision_frame_is_accepted() {
        let Ok(Request::Provision(original)) = parse(provision().to_string().as_bytes()) else {
            panic!("the frame must be accepted");
        };
        let line = provision_frame(&original.config, KEY_A, None).unwrap();
        assert!(line.ends_with('\n'));
        let Ok(Request::Provision(read)) = parse(line.trim_end().as_bytes()) else {
            panic!("the built frame must be accepted");
        };
        assert_eq!(read.config, original.config);
        assert_eq!(read.key.as_str(), KEY_A);
        assert!(read.cascade_key.is_none());

        let mut cascaded = original.config.clone();
        cascaded.link_cascade = true;
        let line = provision_frame(&cascaded, KEY_A, Some(KEY_B)).unwrap();
        let Ok(Request::Provision(read)) = parse(line.trim_end().as_bytes()) else {
            panic!("the built frame must be accepted");
        };
        assert_eq!(read.config, cascaded);
        assert_eq!(
            read.cascade_key.map(|key| key.as_str().to_string()),
            Some(KEY_B.into())
        );

        assert!(provision_frame(&original.config, "a\"b", None).is_err());
    }
}
