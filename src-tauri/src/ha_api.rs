//! Home Assistant REST client (the optional PC -> HA channel).
//! Used by hotkeys (toggle/service actions), widgets (entity states), and the tray.
//! The long-lived token lives EXCLUSIVELY in the user's keyring (Windows Credential
//! Manager, the Secret Service on Linux), never in config.json. Local URL + fallback URL (failover, same as the MQTT broker).
//! ureq + native-tls (schannel) - zero ring/clang, works on ARM64.

#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "the state listings serve the Stream Deck channel, a Windows named pipe"
    )
)]
#![allow(
    clippy::result_large_err,
    reason = "ureq::Error is large by design, and every call here is a single HTTP request"
)]

use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::config::AppConfig;

/// Service name in Credential Manager for the HA token (separate from the MQTT password).
const KEYRING_SERVICE_HA: &str = crate::consts::HA_TOKEN_KEYRING_SERVICE;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: serde_json::Value,
}

pub fn set_token(cfg: &AppConfig, token: &str) -> Result<(), String> {
    let store = deskmate_core::secrets::selected();
    if token.is_empty() {
        if let Err(e) = store.delete(KEYRING_SERVICE_HA, &cfg.node_id) {
            log::warn!("cannot remove the Home Assistant token: {e}");
        }
        return Ok(());
    }
    store.set(KEYRING_SERVICE_HA, &cfg.node_id, token)
}

pub fn get_token(cfg: &AppConfig) -> Option<String> {
    deskmate_core::secrets::selected()
        .get(KEYRING_SERVICE_HA, &cfg.node_id)
        .ok()
        .flatten()
        .map(|token| token.as_str().to_owned())
}

pub fn has_token(cfg: &AppConfig) -> bool {
    get_token(cfg).is_some()
}

/// Whether the API channel is configured (URL + token).
pub fn is_configured(cfg: &AppConfig) -> bool {
    !cfg.ha_url.trim().is_empty() && has_token(cfg)
}

/// What a request to Home Assistant may take, connection included.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
/// ureq 2's default, kept when moving to ureq 3 (which follows 10).
const MAX_REDIRECTS: u32 = 5;

fn agent() -> ureq::Agent {
    crate::http_client::agent(REQUEST_TIMEOUT, MAX_REDIRECTS)
}

/// URL candidates in try order: local first, then the fallback (if different).
/// Normalizes a configured HA base URL before an Authorization header can be
/// sent to it. HTTP remains supported for a private LAN for compatibility; a
/// public/fallback HTTPS-only policy is a deliberate product decision.
pub fn normalize_base_url(input: &str) -> Result<String, String> {
    let value = input.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let url = Url::parse(value).map_err(|_| "HA API: invalid URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
        return Err("HA API: URL must be absolute http/https".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("HA API: URL must not contain embedded credentials".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("HA API: URL must not contain a query or fragment".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn require_https_base_url(input: &str) -> Result<String, String> {
    let normalized = normalize_base_url(input)?;
    if !normalized.is_empty() && !normalized.starts_with("https://") {
        return Err("HA API fallback URL must use HTTPS".into());
    }
    Ok(normalized)
}

fn urls(cfg: &AppConfig) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let local = normalize_base_url(&cfg.ha_url)?;
    if !local.is_empty() {
        out.push(local.clone());
    }
    let remote = normalize_base_url(&cfg.ha_url_remote)?;
    if !remote.is_empty() && remote != local {
        out.push(remote);
    }
    Ok(out)
}

/// Runs the request against the first URL that responds (failover on transport errors).
/// An HTTP error (4xx/5xx) does NOT switch hosts - that's a server response (e.g. a bad token).
fn with_failover<T>(
    cfg: &AppConfig,
    f: impl Fn(&ureq::Agent, &str, &str) -> Result<T, ureq::Error>,
) -> Result<T, String> {
    let token = get_token(cfg).ok_or(
        "No Home Assistant API token saved. This optional channel is only for hotkeys, \
         widgets and tray actions; the connection to Home Assistant itself is shown on the \
         Status page",
    )?;
    let candidates = urls(cfg)?;
    if candidates.is_empty() {
        return Err("HA API: no URL configured".into());
    }
    let agent = agent();
    let mut last_err = String::new();
    for base in &candidates {
        match f(&agent, base, &token) {
            Ok(v) => return Ok(v),
            Err(ureq::Error::StatusCode(code)) => {
                return Err(format!("HA API returned HTTP {code}"))
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(format!("HA API unreachable: {last_err}"))
}

/// POST /api/services/{domain}/{service}. data = extra JSON fields (can be an empty object).
pub fn call_service(
    cfg: &AppConfig,
    domain: &str,
    service: &str,
    entity_id: Option<&str>,
    data: &serde_json::Value,
) -> Result<(), String> {
    // validation: [a-z0-9_] - this ends up in the URL path
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    if !ok(domain) || !ok(service) {
        return Err(format!("invalid service: {domain}.{service}"));
    }
    let mut body = if data.is_object() {
        data.clone()
    } else {
        serde_json::json!({})
    };
    if let Some(id) = entity_id {
        if !id.trim().is_empty() {
            body["entity_id"] = serde_json::json!(id.trim());
        }
    }
    with_failover(cfg, |agent, base, token| {
        agent
            .post(&format!("{base}/api/services/{domain}/{service}"))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map(|_| ())
    })
}

/// homeassistant.toggle on an entity - the most common hotkey/widget action.
pub fn toggle(cfg: &AppConfig, entity_id: &str) -> Result<(), String> {
    call_service(
        cfg,
        "homeassistant",
        "toggle",
        Some(entity_id),
        &serde_json::json!({}),
    )
}

/// GET /api/states/{entity_id} - a single entity (widgets).
pub fn get_state(cfg: &AppConfig, entity_id: &str) -> Result<EntityState, String> {
    let id = entity_id.trim().to_string();
    if !is_valid_entity_id(&id) {
        return Err("invalid entity_id".into());
    }
    let raw = with_failover(cfg, |agent, base, token| {
        agent
            .get(&format!("{base}/api/states/{id}"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map(|mut r| r.body_mut().read_to_string().unwrap_or_default())
    })?;
    serde_json::from_str::<EntityState>(&raw).map_err(|e| e.to_string())
}

/// One entry of `GET /api/states`, reduced to what a list of entities shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitySummary {
    pub entity_id: String,
    pub state: String,
    pub friendly_name: Option<String>,
}

/// A list of states larger than this is not read. A large installation
/// answers with a few megabytes.
const MAX_STATES_BYTES: u64 = 32 * 1024 * 1024;

/// GET /api/states - every entity with its state and friendly name (the
/// entity lists of the Stream Deck channel).
pub fn get_all_states(cfg: &AppConfig) -> Result<Vec<EntitySummary>, String> {
    use std::io::Read;
    let raw = with_failover(cfg, |agent, base, token| {
        let response = agent
            .get(&format!("{base}/api/states"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()?;
        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_STATES_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(ureq::Error::from)?;
        Ok(body)
    })?;
    if raw.len() as u64 > MAX_STATES_BYTES {
        return Err("HA API: the list of states is too large".into());
    }
    parse_states(&raw)
}

fn parse_states(raw: &[u8]) -> Result<Vec<EntitySummary>, String> {
    // Only these fields are kept; the other attributes of an entity can be
    // large and are skipped while parsing.
    #[derive(Deserialize)]
    struct Listed {
        entity_id: String,
        state: String,
        #[serde(default)]
        attributes: ListedAttributes,
    }
    #[derive(Deserialize, Default)]
    struct ListedAttributes {
        #[serde(default)]
        friendly_name: serde_json::Value,
    }
    let listed: Vec<Listed> =
        serde_json::from_slice(raw).map_err(|_| "HA API: unreadable list of states".to_string())?;
    Ok(listed
        .into_iter()
        .map(|entry| EntitySummary {
            entity_id: entry.entity_id,
            state: entry.state,
            friendly_name: entry.attributes.friendly_name.as_str().map(str::to_string),
        })
        .collect())
}

/// The states of several entities, asked over one connection (the watched
/// entities of the Stream Deck channel). `None`: Home Assistant has no such
/// entity. Unlike `get_states_for`, a failure to reach Home Assistant is an
/// error and not an empty answer.
pub fn get_states_of(cfg: &AppConfig, ids: &[String]) -> Result<Vec<Option<String>>, String> {
    if ids.iter().any(|id| !is_valid_entity_id(id)) {
        return Err("invalid entity_id".into());
    }
    let bodies = with_failover(cfg, |agent, base, token| {
        let mut bodies = Vec::with_capacity(ids.len());
        for id in ids {
            let answer = agent
                .get(&format!("{base}/api/states/{id}"))
                .header("Authorization", &format!("Bearer {token}"))
                .call();
            match answer {
                Ok(mut response) => {
                    bodies.push(Some(response.body_mut().read_to_string()?));
                }
                Err(ureq::Error::StatusCode(404)) => bodies.push(None),
                Err(error) => return Err(error),
            }
        }
        Ok(bodies)
    })?;
    bodies
        .into_iter()
        .map(|body| match body {
            None => Ok(None),
            Some(raw) => serde_json::from_str::<EntityState>(&raw)
                .map(|entity| Some(entity.state))
                .map_err(|_| "HA API: unreadable state".to_string()),
        })
        .collect()
}

fn is_valid_entity_id(id: &str) -> bool {
    let Some((domain, object_id)) = id.split_once('.') else {
        return false;
    };
    !domain.is_empty()
        && !object_id.is_empty()
        && !object_id.contains('.')
        && domain
            .chars()
            .chain(object_id.chars())
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// States of multiple entities at once (widget polling). Missing entities are skipped.
pub fn get_states_for(cfg: &AppConfig, ids: &[String]) -> Vec<EntityState> {
    ids.iter()
        .filter_map(|id| get_state(cfg, id).ok())
        .collect()
}

/// Quick connectivity test (GET /api/ returns {"message": "API running."}).
pub fn ping(cfg: &AppConfig) -> Result<String, String> {
    with_failover(cfg, |agent, base, token| {
        agent
            .get(&format!("{base}/api/"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map(|mut r| r.body_mut().read_to_string().unwrap_or_default())
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_states, EntitySummary};

    #[test]
    fn the_list_of_states_keeps_id_state_and_friendly_name() {
        let raw = br#"[
            {"entity_id": "light.desk", "state": "on",
             "attributes": {"friendly_name": "Desk", "brightness": 255, "rgb_color": [1, 2, 3]},
             "last_changed": "2026-01-01T00:00:00+00:00", "context": {"id": "x"}},
            {"entity_id": "sun.sun", "state": "below_horizon", "attributes": {}},
            {"entity_id": "sensor.odd", "state": "1", "attributes": {"friendly_name": 7}},
            {"entity_id": "sensor.bare", "state": "2"}
        ]"#;
        let summary = |entity_id: &str, state: &str, friendly_name: Option<&str>| EntitySummary {
            entity_id: entity_id.into(),
            state: state.into(),
            friendly_name: friendly_name.map(Into::into),
        };
        assert_eq!(
            parse_states(raw).unwrap(),
            [
                summary("light.desk", "on", Some("Desk")),
                summary("sun.sun", "below_horizon", None),
                summary("sensor.odd", "1", None),
                summary("sensor.bare", "2", None),
            ]
        );
    }

    #[test]
    fn an_answer_that_is_not_a_list_of_states_is_an_error() {
        for raw in [
            &b"{}"[..],
            b"[1]",
            b"[{\"state\": \"on\"}]",
            b"not json",
            b"",
        ] {
            assert!(parse_states(raw).is_err());
        }
        assert_eq!(parse_states(b"[]").unwrap(), []);
    }
}
