//! What the channel needs from the rest of the app, as traits. The app
//! implements them with `ha_api` and `actions::execute` (`app.rs`); the tests
//! use stand-ins, so that no test reaches Home Assistant or runs an action.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};

/// How often the watched entities are read from Home Assistant while a
/// connection watches any.
const WATCH_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// After a `toggle` or a `call` went through, the watched entities are read
/// again this soon, so that a key shows its new state without waiting for the
/// next regular read.
const AFTER_ACTION_POLL_DELAY: Duration = Duration::from_millis(400);
/// While the pipe name cannot be claimed, the first tries come this quickly:
/// a client that has not yet noticed the end of the previous channel keeps the
/// name alive for a moment.
const NAME_CLAIM_GRACE_DELAY: Duration = Duration::from_millis(200);
/// Once the name counts as taken by another program, it is tried again at
/// this interval, so the channel comes up when that program goes away.
const NAME_CLAIM_RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// One entity as the channel lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityInfo {
    pub entity_id: String,
    /// The friendly name, or the entity id when there is none.
    pub name: String,
    pub state: String,
}

/// A hotkey or a tray quick action as the channel lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEntry {
    /// `hotkey:<id>` or `tray:<id>`.
    pub id: String,
    pub name: String,
}

/// A service call, every part already validated.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceCall {
    pub domain: String,
    pub service: String,
    pub entity_id: Option<String>,
    pub data: Map<String, Value>,
}

/// Home Assistant could not be reached or refused the request. The details
/// stay with the implementation; the client only learns `ha_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HaFailure;

/// Home Assistant, as far as the channel uses it.
pub trait HomeAssistant: Send + Sync + 'static {
    /// Whether the app has an address and a token for Home Assistant.
    fn is_configured(&self) -> impl Future<Output = bool> + Send;

    /// Every entity Home Assistant has.
    fn entities(&self) -> impl Future<Output = Result<Vec<EntityInfo>, HaFailure>> + Send;

    /// The states of the given entities. An entity Home Assistant does not
    /// have is missing from the answer.
    fn states(
        &self,
        entity_ids: &[String],
    ) -> impl Future<Output = Result<HashMap<String, String>, HaFailure>> + Send;

    /// `homeassistant.toggle` on one entity.
    fn toggle(&self, entity_id: &str) -> impl Future<Output = Result<(), HaFailure>> + Send;

    fn call_service(&self, call: ServiceCall)
        -> impl Future<Output = Result<(), HaFailure>> + Send;
}

/// The actions the user defined in Deskmate: hotkeys and tray quick actions.
pub trait ActionRunner: Send + Sync + 'static {
    fn actions(&self) -> impl Future<Output = Vec<ActionEntry>> + Send;

    /// Runs the action with this id and returns once it has run. `false`:
    /// there is no such action.
    fn run(&self, action_id: &str) -> impl Future<Output = bool> + Send;
}

/// Where the channel records what it did. The app writes the security log.
pub trait AuditSink: Send + Sync + 'static {
    fn record(&self, event: &str, result: &str);
}

/// The waits of the channel. Tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub watch_poll_interval: Duration,
    pub after_action_poll_delay: Duration,
    pub name_claim_grace_delay: Duration,
    pub name_claim_retry_interval: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            watch_poll_interval: WATCH_POLL_INTERVAL,
            after_action_poll_delay: AFTER_ACTION_POLL_DELAY,
            name_claim_grace_delay: NAME_CLAIM_GRACE_DELAY,
            name_claim_retry_interval: NAME_CLAIM_RETRY_INTERVAL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelState {
    Off,
    /// Switched on; the pipe is not open yet.
    Starting,
    Listening,
    /// Another program holds the pipe name. The app does not join its pipe.
    NameTaken,
    /// The pipe could not be opened for another reason.
    Failed,
}

/// What the Settings panel shows about the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ChannelStatus {
    pub state: ChannelState,
    /// Clients connected right now.
    pub connected: usize,
}

/// Everything a connection works with.
pub struct Backend<H, A> {
    pub home_assistant: H,
    pub actions: A,
    pub audit: Arc<dyn AuditSink>,
    /// The Deskmate version, sent in the answer to `hello`.
    pub app_version: String,
    pub timing: Timing,
}
