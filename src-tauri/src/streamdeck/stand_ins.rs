//! Stand-ins for the tests: Home Assistant, the action executor and the
//! audit log, all in memory, and a client that speaks the channel's lines.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadHalf,
    WriteHalf,
};
use tokio::sync::Notify;

use super::backend::{
    ActionEntry, ActionRunner, AuditSink, Backend, EntityInfo, HaFailure, HomeAssistant,
    ServiceCall, Timing,
};

/// How long a test waits for something that has to happen.
pub const PATIENCE: Duration = Duration::from_secs(10);

/// A Home Assistant that lives in memory.
#[derive(Clone, Default)]
pub struct FakeHomeAssistant(Arc<FakeHomeAssistantState>);

#[derive(Default)]
pub struct FakeHomeAssistantState {
    not_configured: AtomicBool,
    unreachable: AtomicBool,
    entities: Mutex<Vec<EntityInfo>>,
    /// Every request that reached it, in order.
    pub requests: Mutex<Vec<String>>,
    /// How often the states of watched entities were read.
    pub state_reads: AtomicUsize,
    /// While set, `entities` does not answer until it is notified.
    entities_gate: Mutex<Option<Arc<Notify>>>,
    /// While set, `toggle` reaches Home Assistant but does not return until
    /// it is notified.
    toggle_gate: Mutex<Option<Arc<Notify>>>,
}

impl FakeHomeAssistant {
    pub fn with_entities(entities: &[(&str, &str, &str)]) -> Self {
        let fake = Self::default();
        *fake.0.entities.lock().unwrap() = entities
            .iter()
            .map(|(entity_id, name, state)| EntityInfo {
                entity_id: entity_id.to_string(),
                name: name.to_string(),
                state: state.to_string(),
            })
            .collect();
        fake
    }

    pub fn set_configured(&self, configured: bool) {
        self.0.not_configured.store(!configured, Ordering::SeqCst);
    }

    pub fn set_reachable(&self, reachable: bool) {
        self.0.unreachable.store(!reachable, Ordering::SeqCst);
    }

    pub fn set_entities(&self, entities: Vec<EntityInfo>) {
        *self.0.entities.lock().unwrap() = entities;
    }

    pub fn set_state(&self, entity_id: &str, state: &str) {
        let mut entities = self.0.entities.lock().unwrap();
        match entities.iter_mut().find(|e| e.entity_id == entity_id) {
            Some(entity) => entity.state = state.to_string(),
            None => entities.push(EntityInfo {
                entity_id: entity_id.to_string(),
                name: entity_id.to_string(),
                state: state.to_string(),
            }),
        }
    }

    pub fn remove(&self, entity_id: &str) {
        self.0
            .entities
            .lock()
            .unwrap()
            .retain(|e| e.entity_id != entity_id);
    }

    /// Makes `entities` wait. The returned handle lets it answer.
    pub fn hold_entities(&self) -> Arc<Notify> {
        let gate = Arc::new(Notify::new());
        *self.0.entities_gate.lock().unwrap() = Some(Arc::clone(&gate));
        gate
    }

    /// Makes `toggle` wait once it has reached Home Assistant. The returned
    /// handle lets it return.
    pub fn hold_toggles(&self) -> Arc<Notify> {
        let gate = Arc::new(Notify::new());
        *self.0.toggle_gate.lock().unwrap() = Some(Arc::clone(&gate));
        gate
    }

    pub fn requests(&self) -> Vec<String> {
        self.0.requests.lock().unwrap().clone()
    }

    pub fn state_reads(&self) -> usize {
        self.0.state_reads.load(Ordering::SeqCst)
    }

    fn reach(&self, request: String) -> Result<(), HaFailure> {
        self.0.requests.lock().unwrap().push(request);
        if self.0.unreachable.load(Ordering::SeqCst) {
            Err(HaFailure)
        } else {
            Ok(())
        }
    }
}

impl HomeAssistant for FakeHomeAssistant {
    async fn is_configured(&self) -> bool {
        !self.0.not_configured.load(Ordering::SeqCst)
    }

    async fn entities(&self) -> Result<Vec<EntityInfo>, HaFailure> {
        let gate = self.0.entities_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.notified().await;
        }
        self.reach("entities".into())?;
        Ok(self.0.entities.lock().unwrap().clone())
    }

    async fn states(&self, entity_ids: &[String]) -> Result<HashMap<String, String>, HaFailure> {
        self.0.state_reads.fetch_add(1, Ordering::SeqCst);
        if self.0.unreachable.load(Ordering::SeqCst) {
            return Err(HaFailure);
        }
        let entities = self.0.entities.lock().unwrap();
        Ok(entities
            .iter()
            .filter(|entity| entity_ids.contains(&entity.entity_id))
            .map(|entity| (entity.entity_id.clone(), entity.state.clone()))
            .collect())
    }

    async fn toggle(&self, entity_id: &str) -> Result<(), HaFailure> {
        self.reach(format!("toggle {entity_id}"))?;
        {
            let mut entities = self.0.entities.lock().unwrap();
            if let Some(entity) = entities.iter_mut().find(|e| e.entity_id == entity_id) {
                entity.state = if entity.state == "on" { "off" } else { "on" }.to_string();
            }
        }
        let gate = self.0.toggle_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.notified().await;
        }
        Ok(())
    }

    async fn call_service(&self, call: ServiceCall) -> Result<(), HaFailure> {
        self.reach(format!(
            "call {}.{} {} {}",
            call.domain,
            call.service,
            call.entity_id.as_deref().unwrap_or("-"),
            Value::Object(call.data)
        ))
    }
}

/// The user's actions, in memory. Running one only records it.
#[derive(Clone, Default)]
pub struct FakeActions(Arc<FakeActionsState>);

type RunHook = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct FakeActionsState {
    actions: Mutex<Vec<ActionEntry>>,
    ran: Mutex<Vec<String>>,
    on_run: Mutex<Option<RunHook>>,
    /// While set, `run` records the action but does not return until it is
    /// notified.
    run_gate: Mutex<Option<Arc<Notify>>>,
}

impl FakeActions {
    pub fn with_actions(actions: &[(&str, &str)]) -> Self {
        let fake = Self::default();
        fake.set_actions(
            actions
                .iter()
                .map(|(id, name)| ActionEntry {
                    id: id.to_string(),
                    name: name.to_string(),
                })
                .collect(),
        );
        fake
    }

    pub fn set_actions(&self, actions: Vec<ActionEntry>) {
        *self.0.actions.lock().unwrap() = actions;
    }

    /// Called with the id of every action that is run.
    pub fn on_run(&self, hook: impl Fn(&str) + Send + Sync + 'static) {
        *self.0.on_run.lock().unwrap() = Some(Box::new(hook));
    }

    /// Makes `run` wait once it has recorded the action. The returned handle
    /// lets it return.
    pub fn hold_runs(&self) -> Arc<Notify> {
        let gate = Arc::new(Notify::new());
        *self.0.run_gate.lock().unwrap() = Some(Arc::clone(&gate));
        gate
    }

    pub fn ran(&self) -> Vec<String> {
        self.0.ran.lock().unwrap().clone()
    }
}

impl ActionRunner for FakeActions {
    async fn actions(&self) -> Vec<ActionEntry> {
        self.0.actions.lock().unwrap().clone()
    }

    async fn run(&self, action_id: &str) -> bool {
        let known = self
            .0
            .actions
            .lock()
            .unwrap()
            .iter()
            .any(|action| action.id == action_id);
        if known {
            self.0.ran.lock().unwrap().push(action_id.to_string());
            if let Some(hook) = self.0.on_run.lock().unwrap().as_ref() {
                hook(action_id);
            }
        }
        let gate = self.0.run_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.notified().await;
        }
        known
    }
}

/// An audit log that keeps its records as `event result` lines.
#[derive(Default)]
pub struct RecordedAudit(Mutex<Vec<String>>);

impl RecordedAudit {
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl AuditSink for RecordedAudit {
    fn record(&self, event: &str, result: &str) {
        self.0.lock().unwrap().push(format!("{event} {result}"));
    }
}

/// Waits that keep the tests quick.
pub fn quick_timing() -> Timing {
    Timing {
        watch_poll_interval: Duration::from_millis(30),
        after_action_poll_delay: Duration::from_millis(5),
        name_claim_grace_delay: Duration::from_millis(40),
        name_claim_retry_interval: Duration::from_millis(150),
    }
}

pub type FakeBackend = Backend<FakeHomeAssistant, FakeActions>;

pub fn backend(
    home_assistant: &FakeHomeAssistant,
    actions: &FakeActions,
    timing: Timing,
) -> (Arc<FakeBackend>, Arc<RecordedAudit>) {
    let audit = Arc::new(RecordedAudit::default());
    let backend = Arc::new(Backend {
        home_assistant: home_assistant.clone(),
        actions: actions.clone(),
        audit: Arc::clone(&audit) as Arc<dyn AuditSink>,
        app_version: "0.9.0".into(),
        timing,
    });
    (backend, audit)
}

/// A client of the channel over any stream: an in-memory one in the session
/// tests, a real named pipe in the server tests.
pub struct Client<S> {
    reader: BufReader<ReadHalf<S>>,
    writer: WriteHalf<S>,
    /// Events that arrived while a response was waited for.
    pub events: Vec<Value>,
    next_id: u64,
}

impl<S: AsyncRead + AsyncWrite> Client<S> {
    pub fn new(stream: S) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        Self {
            reader: BufReader::new(reader),
            writer,
            events: Vec::new(),
            next_id: 1,
        }
    }

    pub async fn send_raw(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    pub async fn send(&mut self, message: Value) {
        self.send_raw(format!("{message}\n").as_bytes()).await;
    }

    /// The next line from the app, or `None` when it closed the connection.
    pub async fn next(&mut self) -> Option<Value> {
        let mut line = Vec::new();
        let read = tokio::time::timeout(PATIENCE, self.reader.read_until(b'\n', &mut line))
            .await
            .expect("the app sent nothing in time");
        match read {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                assert_eq!(line.last(), Some(&b'\n'), "a line without its newline");
                assert!(line.len() <= super::framing::MAX_LINE_BYTES + 1);
                Some(serde_json::from_slice(&line).expect("the app sent a line that is not JSON"))
            }
        }
    }

    /// The response with this id. Events that come first are kept.
    pub async fn response(&mut self, id: u64) -> Value {
        loop {
            let message = self.next().await.expect("the app closed the connection");
            if message.get("ev").is_some() {
                self.events.push(message);
            } else if message["id"] == id {
                return message;
            } else {
                panic!("an unexpected response: {message}");
            }
        }
    }

    /// Sends a request with the next id and returns its response.
    pub async fn request(&mut self, mut request: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        request["id"] = Value::from(id);
        self.send(request).await;
        self.response(id).await
    }

    pub async fn hello(&mut self) -> Value {
        self.request(serde_json::json!({
            "op": "hello", "v": 1, "client": "streamdeck", "version": "0.2.0"
        }))
        .await
    }

    /// The next event, from the kept ones first.
    pub async fn event(&mut self) -> Value {
        if !self.events.is_empty() {
            return self.events.remove(0);
        }
        let message = self.next().await.expect("the app closed the connection");
        assert!(message.get("ev").is_some(), "not an event: {message}");
        message
    }

    /// Asserts that the app sends nothing for a while.
    pub async fn expect_silence(&mut self, duration: Duration) {
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(duration, self.reader.read(&mut byte)).await;
        assert!(read.is_err(), "the app sent something, or closed");
    }

    /// Asserts that the app closes the connection without another line.
    pub async fn expect_closed(&mut self) {
        assert_eq!(
            self.next().await,
            None,
            "the app answered instead of closing"
        );
    }
}

/// Waits until the condition holds.
pub async fn eventually(what: &str, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
