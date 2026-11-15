//! The messages of the channel and their validation, as specified in the
//! "Local channel" section of `docs/STREAMDECK.md`. Everything a client sends
//! is checked here before it reaches Home Assistant, an action or a log.

use serde_json::{Map, Number, Value};

use super::backend::ServiceCall;
use super::framing::MAX_LINE_BYTES;

/// Version of the channel this app speaks.
pub const PROTOCOL_VERSION: u64 = 1;

/// A connection watches at most this many entities.
pub const MAX_WATCHED_ENTITIES: usize = 64;

/// Home Assistant's own limit for an entity id.
const MAX_ENTITY_ID_BYTES: usize = 255;
/// Longest domain or service name the channel accepts.
const MAX_NAME_PART_BYTES: usize = 64;
/// `entities` filters by at most this many domains.
const MAX_DOMAINS: usize = 64;
/// Longest action id: a prefix, a colon and the id of a hotkey or tray action.
const MAX_ACTION_ID_BYTES: usize = 128;
/// Longest name a client may give itself, and longest version, in `hello`.
const MAX_CLIENT_FIELD_BYTES: usize = 32;
/// Largest integer JavaScript represents exactly. An `id` beyond it would not
/// survive the trip through the plugin.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

pub type Message = Map<String, Value>;

/// The `id` of a request, echoed in its response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(Number);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Version,
    BadRequest,
    UnknownAction,
    HaNotConfigured,
    HaError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::BadRequest => "bad_request",
            Self::UnknownAction => "unknown_action",
            Self::HaNotConfigured => "ha_not_configured",
            Self::HaError => "ha_error",
        }
    }

    /// English text for the user, for the codes that have something to say.
    fn message(self) -> Option<&'static str> {
        match self {
            Self::Version | Self::BadRequest => None,
            Self::UnknownAction => Some("That action no longer exists."),
            Self::HaNotConfigured => Some("Deskmate has no Home Assistant address and token yet."),
            Self::HaError => Some("Home Assistant could not be reached or refused the request."),
        }
    }
}

/// A request whose fields passed validation.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Hello,
    Actions,
    Run { action: String },
    Entities { domains: Option<Vec<String>> },
    Watch { entities: Vec<String> },
    Toggle { entity_id: String },
    Call(ServiceCall),
}

/// One line from a client that is a JSON object.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Request {
        id: RequestId,
        request: Request,
    },
    /// Answered with the error; nothing is carried out. Without a usable
    /// `id` the answer carries none.
    Refused {
        id: Option<RequestId>,
        error: ErrorCode,
    },
}

/// Reads one line from a client. `None`: the line is not a JSON object, and
/// the connection is closed over it without an answer.
pub fn parse_line(line: &str) -> Option<Incoming> {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(line) else {
        return None;
    };
    let Some(id) = request_id(&fields) else {
        return Some(Incoming::Refused {
            id: None,
            error: ErrorCode::BadRequest,
        });
    };
    Some(match parse_request(&fields) {
        Ok(request) => Incoming::Request { id, request },
        Err(error) => Incoming::Refused {
            id: Some(id),
            error,
        },
    })
}

fn request_id(fields: &Message) -> Option<RequestId> {
    let Value::Number(number) = fields.get("id")? else {
        return None;
    };
    let magnitude = match (number.as_u64(), number.as_i64()) {
        (Some(positive), _) => positive,
        (None, Some(negative)) => negative.unsigned_abs(),
        // A fraction or an exponent: not an integer.
        (None, None) => return None,
    };
    (magnitude <= MAX_SAFE_INTEGER).then(|| RequestId(number.clone()))
}

fn parse_request(fields: &Message) -> Result<Request, ErrorCode> {
    match required_string(fields, "op")? {
        "hello" => parse_hello(fields),
        "actions" => Ok(Request::Actions),
        "run" => {
            let action = required_string(fields, "action")?;
            if !is_valid_action_id(action) {
                return Err(ErrorCode::BadRequest);
            }
            Ok(Request::Run {
                action: action.to_string(),
            })
        }
        "entities" => Ok(Request::Entities {
            domains: parse_domains(fields)?,
        }),
        "watch" => Ok(Request::Watch {
            entities: parse_watch_list(fields)?,
        }),
        "toggle" => {
            let entity_id = required_string(fields, "entity_id")?;
            if !is_valid_entity_id(entity_id) {
                return Err(ErrorCode::BadRequest);
            }
            Ok(Request::Toggle {
                entity_id: entity_id.to_string(),
            })
        }
        "call" => parse_call(fields).map(Request::Call),
        _ => Err(ErrorCode::BadRequest),
    }
}

fn required_string<'a>(fields: &'a Message, key: &str) -> Result<&'a str, ErrorCode> {
    fields
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::BadRequest)
}

fn parse_hello(fields: &Message) -> Result<Request, ErrorCode> {
    let version = fields
        .get("v")
        .and_then(Value::as_u64)
        .ok_or(ErrorCode::BadRequest)?;
    // Checked before the other fields: a later version may spell them
    // differently, and the answer to it is `version` all the same.
    if version != PROTOCOL_VERSION {
        return Err(ErrorCode::Version);
    }
    let client = required_string(fields, "client")?;
    let client_version = required_string(fields, "version")?;
    let named = |text: &str, allowed: fn(char) -> bool| {
        !text.is_empty() && text.len() <= MAX_CLIENT_FIELD_BYTES && text.chars().all(allowed)
    };
    let is_name = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
    let is_version = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+');
    if !named(client, is_name) || !named(client_version, is_version) {
        return Err(ErrorCode::BadRequest);
    }
    Ok(Request::Hello)
}

fn parse_domains(fields: &Message) -> Result<Option<Vec<String>>, ErrorCode> {
    let Some(value) = fields.get("domains") else {
        return Ok(None);
    };
    let Value::Array(items) = value else {
        return Err(ErrorCode::BadRequest);
    };
    if items.len() > MAX_DOMAINS {
        return Err(ErrorCode::BadRequest);
    }
    let mut domains = Vec::with_capacity(items.len());
    for item in items {
        let domain = item.as_str().ok_or(ErrorCode::BadRequest)?;
        if !is_valid_name_part(domain) {
            return Err(ErrorCode::BadRequest);
        }
        domains.push(domain.to_string());
    }
    Ok(Some(domains))
}

fn parse_watch_list(fields: &Message) -> Result<Vec<String>, ErrorCode> {
    let Some(Value::Array(items)) = fields.get("entities") else {
        return Err(ErrorCode::BadRequest);
    };
    if items.len() > MAX_WATCHED_ENTITIES {
        return Err(ErrorCode::BadRequest);
    }
    let mut entities: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let entity_id = item.as_str().ok_or(ErrorCode::BadRequest)?;
        if !is_valid_entity_id(entity_id) {
            return Err(ErrorCode::BadRequest);
        }
        if !entities.iter().any(|known| known == entity_id) {
            entities.push(entity_id.to_string());
        }
    }
    Ok(entities)
}

fn parse_call(fields: &Message) -> Result<ServiceCall, ErrorCode> {
    let (domain, service) =
        parse_service(required_string(fields, "service")?).ok_or(ErrorCode::BadRequest)?;
    let entity_id = match fields.get("entity_id") {
        None => None,
        Some(Value::String(entity_id)) if is_valid_entity_id(entity_id) => Some(entity_id.clone()),
        Some(_) => return Err(ErrorCode::BadRequest),
    };
    let data = match fields.get("data") {
        None => Map::new(),
        Some(Value::Object(data)) => data.clone(),
        Some(_) => return Err(ErrorCode::BadRequest),
    };
    Ok(ServiceCall {
        domain: domain.to_string(),
        service: service.to_string(),
        entity_id,
        data,
    })
}

/// A domain, a service name or the object part of an entity id.
fn is_valid_name_part(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_NAME_PART_BYTES
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `domain.object_id`, both from a-z, 0-9 and `_`, as Home Assistant writes
/// its entity ids.
pub fn is_valid_entity_id(entity_id: &str) -> bool {
    if entity_id.len() > MAX_ENTITY_ID_BYTES {
        return false;
    }
    let Some((domain, object_id)) = entity_id.split_once('.') else {
        return false;
    };
    let is_part = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    is_part(domain) && is_part(object_id)
}

/// `domain.service`, split into its two parts.
fn parse_service(text: &str) -> Option<(&str, &str)> {
    let (domain, service) = text.split_once('.')?;
    (is_valid_name_part(domain) && is_valid_name_part(service)).then_some((domain, service))
}

/// Whether the text is shaped like an action id. An id of this shape that
/// names no action is answered with `unknown_action`; any other text is a
/// malformed field. The shape is strict because the id goes into the audit
/// log.
pub fn is_valid_action_id(action_id: &str) -> bool {
    !action_id.is_empty()
        && action_id.len() <= MAX_ACTION_ID_BYTES
        && action_id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == ':')
}

/// A successful response: `id`, `ok` and the fields of the body.
pub fn success(id: &RequestId, mut body: Message) -> Message {
    body.insert("id".into(), Value::Number(id.0.clone()));
    body.insert("ok".into(), Value::Bool(true));
    body
}

/// A refusal or a failure.
pub fn failure(id: Option<&RequestId>, error: ErrorCode) -> Message {
    let mut message = Message::new();
    if let Some(id) = id {
        message.insert("id".into(), Value::Number(id.0.clone()));
    }
    message.insert("ok".into(), Value::Bool(false));
    message.insert("error".into(), Value::from(error.as_str()));
    if let Some(text) = error.message() {
        message.insert("message".into(), Value::from(text));
    }
    message
}

/// The answer to `hello`.
pub fn hello_response(id: &RequestId, app_version: &str, ha_configured: bool) -> Message {
    let mut body = Message::new();
    body.insert("v".into(), Value::from(PROTOCOL_VERSION));
    body.insert("app".into(), Value::from(app_version));
    body.insert("ha".into(), Value::Bool(ha_configured));
    success(id, body)
}

/// The event sent when a watched entity changes.
pub fn state_event(entity_id: &str, state: &str) -> Message {
    let mut event = Message::new();
    event.insert("ev".into(), Value::from("state"));
    event.insert("entity_id".into(), Value::from(entity_id));
    event.insert("state".into(), Value::from(state));
    event
}

/// A successful response that carries a list under `key`. The response has
/// to fit in one line, so the items that do not fit next to the others are
/// left out.
pub fn list_response(id: &RequestId, key: &str, items: Vec<Value>) -> Message {
    let mut body = Message::new();
    body.insert(key.into(), Value::Array(Vec::new()));
    let mut response = success(id, body);
    let mut budget = MAX_LINE_BYTES.saturating_sub(serialized_len(&response));

    let mut kept = Vec::new();
    for item in items {
        // Every item after the first is preceded by a comma.
        let cost = serialized_len(&item).saturating_add(usize::from(!kept.is_empty()));
        if cost <= budget {
            budget -= cost;
            kept.push(item);
        }
    }
    response.insert(key.into(), Value::Array(kept));
    response
}

/// Bytes of the compact JSON form. A value that cannot be serialized counts
/// as larger than any line, so it is left out.
fn serialized_len<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
