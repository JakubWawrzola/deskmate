//! Home Assistant REST client (the optional PC -> HA channel).
//! Used by hotkeys (toggle/service actions), widgets (entity states), and the tray.
//! The long-lived token lives EXCLUSIVELY in Windows Credential Manager (keyring),
//! never in config.json. Local URL + fallback URL (failover, same as the MQTT broker).
//! ureq + native-tls (schannel) - zero ring/clang, works on ARM64.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::config::AppConfig;

/// Service name in Credential Manager for the HA token (separate from the MQTT password).
const KEYRING_SERVICE_HA: &str = "Deskmate HA Token";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: serde_json::Value,
}

pub fn set_token(cfg: &AppConfig, token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE_HA, &cfg.node_id).map_err(|e| e.to_string())?;
    if token.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(token).map_err(|e| e.to_string())
}

pub fn get_token(cfg: &AppConfig) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE_HA, &cfg.node_id)
        .ok()?
        .get_password()
        .ok()
}

pub fn has_token(cfg: &AppConfig) -> bool {
    get_token(cfg).is_some()
}

/// Whether the API channel is configured (URL + token).
pub fn is_configured(cfg: &AppConfig) -> bool {
    !cfg.ha_url.trim().is_empty() && has_token(cfg)
}

fn agent() -> ureq::Agent {
    let connector = native_tls::TlsConnector::new().expect("tls");
    ureq::AgentBuilder::new()
        .tls_connector(std::sync::Arc::new(connector))
        .timeout(Duration::from_secs(6))
        .build()
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
    let token = get_token(cfg).ok_or("HA API: no token configured")?;
    let candidates = urls(cfg)?;
    if candidates.is_empty() {
        return Err("HA API: no URL configured".into());
    }
    let agent = agent();
    let mut last_err = String::new();
    for base in &candidates {
        match f(&agent, base, &token) {
            Ok(v) => return Ok(v),
            Err(ureq::Error::Status(code, _)) => return Err(format!("HA API returned HTTP {code}")),
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
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok(domain) || !ok(service) {
        return Err(format!("invalid service: {domain}.{service}"));
    }
    let mut body = if data.is_object() { data.clone() } else { serde_json::json!({}) };
    if let Some(id) = entity_id {
        if !id.trim().is_empty() {
            body["entity_id"] = serde_json::json!(id.trim());
        }
    }
    with_failover(cfg, |agent, base, token| {
        agent
            .post(&format!("{base}/api/services/{domain}/{service}"))
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map(|_| ())
    })
}

/// homeassistant.toggle on an entity - the most common hotkey/widget action.
pub fn toggle(cfg: &AppConfig, entity_id: &str) -> Result<(), String> {
    call_service(cfg, "homeassistant", "toggle", Some(entity_id), &serde_json::json!({}))
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
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .map(|r| r.into_string().unwrap_or_default())
    })?;
    serde_json::from_str::<EntityState>(&raw).map_err(|e| e.to_string())
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
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .map(|r| r.into_string().unwrap_or_default())
    })
}
