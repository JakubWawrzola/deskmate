//! One connection of the channel: reads requests, answers each one, and
//! sends `state` events for the entities the client watches.
//!
//! The connection is served by a single task. Requests that reach Home
//! Assistant or run an action are carried out next to each other, so that a
//! slow answer from Home Assistant does not hold up a key press; their
//! responses are written as they finish.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;

use super::action_ids;
use super::backend::{ActionRunner, Backend, HaFailure, HomeAssistant, ServiceCall};
use super::entities;
use super::framing::{encode_line, Frame, LineDecoder};
use super::protocol::{
    failure, hello_response, list_response, parse_line, state_event, success, ErrorCode, Incoming,
    Message, Request, RequestId,
};
use super::watch::{states_of, Watch};

/// Audit event names. The result names the action, entity or service and
/// how it went, never anything else from the request.
pub const AUDIT_RUN: &str = "streamdeck_run";
pub const AUDIT_TOGGLE: &str = "streamdeck_toggle";
pub const AUDIT_CALL: &str = "streamdeck_call";

/// Requests of one connection that may be in progress at a time. Further
/// requests wait, the later ones in the pipe, until one has been answered.
const MAX_REQUESTS_IN_PROGRESS: usize = 16;
/// Bytes read from the pipe at a time.
const READ_CHUNK_BYTES: usize = 8 * 1024;

type InProgress = Pin<Box<dyn Future<Output = Finished> + Send>>;

/// What a piece of work on behalf of the connection ended with.
enum Finished {
    Reply(Message),
    /// A `toggle` or a `call`.
    Acted {
        reply: Message,
        succeeded: bool,
    },
    /// The read of Home Assistant behind a `watch` request.
    WatchRead {
        id: RequestId,
        generation: u64,
        entities: Vec<String>,
        read: Result<HashMap<String, String>, HaFailure>,
    },
    /// A regular read of the watched entities.
    Polled {
        generation: u64,
        read: Result<HashMap<String, String>, HaFailure>,
    },
}

struct Session<H, A> {
    backend: Arc<Backend<H, A>>,
    /// A `hello` has succeeded on this connection.
    greeted: bool,
    /// Read once, at `hello`. The app closes its connections when the Home
    /// Assistant settings change, so the value holds for the connection.
    ha_configured: bool,
    watch: Watch,
    /// `watch` requests whose read of Home Assistant has not come back.
    watch_reads: usize,
    poll_in_progress: bool,
    next_poll: Option<Instant>,
    in_progress: FuturesUnordered<InProgress>,
}

/// Serves one connection until the client closes it or breaks the framing.
pub async fn run_session<S, H, A>(stream: S, backend: Arc<Backend<H, A>>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
    H: HomeAssistant,
    A: ActionRunner,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut session = Session {
        backend,
        greeted: false,
        ha_configured: false,
        watch: Watch::default(),
        watch_reads: 0,
        poll_in_progress: false,
        next_poll: None,
        in_progress: FuturesUnordered::new(),
    };
    let mut decoder = LineDecoder::default();
    let mut chunk = vec![0u8; READ_CHUNK_BYTES];
    // Lines that were read and wait for a free place among the requests in
    // progress. Nothing more is read from the pipe while any wait.
    let mut waiting: VecDeque<Frame> = VecDeque::new();

    loop {
        while session.in_progress.len() < MAX_REQUESTS_IN_PROGRESS {
            match waiting.pop_front() {
                None => break,
                Some(Frame::Broken(_)) => return,
                Some(Frame::Line(line)) => {
                    if !session.accept_line(&line).await {
                        return;
                    }
                }
            }
        }
        let poll_due = session.poll_due();
        tokio::select! {
            read = reader.read(&mut chunk), if waiting.is_empty() => {
                let Ok(count) = read else { return };
                if count == 0 {
                    return;
                }
                waiting.extend(decoder.push(&chunk[..count]));
            }
            Some(finished) = session.in_progress.next() => {
                for message in session.finish(finished) {
                    if write_message(&mut writer, &message).await.is_err() {
                        return;
                    }
                }
            }
            () = wait_until(poll_due), if poll_due.is_some() => session.start_poll(),
        }
    }
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Writes one message as one line. A response too long for a line can only
/// come from what Home Assistant reported, so it is answered as a failure of
/// Home Assistant; an event too long for a line is not sent.
async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &Message,
) -> std::io::Result<()> {
    let line = match encode_line(message) {
        Ok(line) => line,
        Err(_) => {
            let Some(id) = message.get("id").cloned() else {
                return Ok(());
            };
            let mut fallback = failure(None, ErrorCode::HaError);
            fallback.insert("id".into(), id);
            match encode_line(&fallback) {
                Ok(line) => line,
                Err(_) => return Ok(()),
            }
        }
    };
    writer.write_all(&line).await?;
    writer.flush().await
}

impl<H: HomeAssistant, A: ActionRunner> Session<H, A> {
    /// Takes one line from the client. `false`: the line is not a JSON
    /// object and the connection is closed.
    async fn accept_line(&mut self, line: &str) -> bool {
        match parse_line(line) {
            None => return false,
            Some(Incoming::Refused { id, error }) => self.reply(failure(id.as_ref(), error)),
            Some(Incoming::Request { id, request }) => self.accept(id, request).await,
        }
        true
    }

    async fn accept(&mut self, id: RequestId, request: Request) {
        if request == Request::Hello {
            self.ha_configured = self.backend.home_assistant.is_configured().await;
            self.greeted = true;
            let response = hello_response(&id, &self.backend.app_version, self.ha_configured);
            self.reply(response);
            return;
        }
        if !self.greeted {
            self.reply(failure(Some(&id), ErrorCode::BadRequest));
            return;
        }
        match request {
            // Answered above.
            Request::Hello => {}
            Request::Actions => self.list_actions(id),
            Request::Run { action } => self.run_action(id, action),
            Request::Entities { domains } => self.list_entities(id, domains),
            Request::Watch { entities } => self.watch(id, entities),
            Request::Toggle { entity_id } => self.toggle(id, entity_id),
            Request::Call(call) => self.call(id, call),
        }
    }

    fn reply(&mut self, message: Message) {
        self.start(async move { Finished::Reply(message) });
    }

    fn start(&mut self, work: impl Future<Output = Finished> + Send + 'static) {
        self.in_progress.push(Box::pin(work));
    }

    /// Answers a request that needs Home Assistant when the app has no
    /// address and token for it. `true`: answered.
    fn refuse_without_home_assistant(&mut self, id: &RequestId) -> bool {
        if self.ha_configured {
            return false;
        }
        self.reply(failure(Some(id), ErrorCode::HaNotConfigured));
        true
    }

    fn list_actions(&mut self, id: RequestId) {
        let backend = Arc::clone(&self.backend);
        self.start(async move {
            let actions = backend.actions.actions().await;
            let items = actions.iter().map(action_ids::to_json).collect();
            Finished::Reply(list_response(&id, "actions", items))
        });
    }

    fn run_action(&mut self, id: RequestId, action: String) {
        let backend = Arc::clone(&self.backend);
        self.start(detached(id.clone(), async move {
            let found = backend.actions.run(&action).await;
            let (reply, result) = if found {
                (success(&id, Message::new()), "ok")
            } else {
                let error = ErrorCode::UnknownAction;
                (failure(Some(&id), error), error.as_str())
            };
            backend
                .audit
                .record(AUDIT_RUN, &format!("action={action} result={result}"));
            Finished::Reply(reply)
        }));
    }

    fn list_entities(&mut self, id: RequestId, domains: Option<Vec<String>>) {
        if self.refuse_without_home_assistant(&id) {
            return;
        }
        let backend = Arc::clone(&self.backend);
        self.start(async move {
            Finished::Reply(match backend.home_assistant.entities().await {
                Ok(all) => {
                    let selected = entities::select(all, domains.as_deref());
                    let items = selected.iter().map(entities::to_json).collect();
                    list_response(&id, "entities", items)
                }
                Err(HaFailure) => failure(Some(&id), ErrorCode::HaError),
            })
        });
    }

    fn watch(&mut self, id: RequestId, entities: Vec<String>) {
        if entities.is_empty() {
            self.watch.replace(entities);
            self.next_poll = None;
            let mut body = Message::new();
            body.insert("states".into(), Value::Object(Message::new()));
            self.reply(success(&id, body));
            return;
        }
        if self.refuse_without_home_assistant(&id) {
            return;
        }
        // The list is replaced now, not when Home Assistant has answered: if
        // it cannot be reached, the app keeps asking and tells the client the
        // states once it can.
        let generation = self.watch.replace(entities.clone());
        self.watch_reads += 1;
        let backend = Arc::clone(&self.backend);
        self.start(async move {
            let read = backend.home_assistant.states(&entities).await;
            Finished::WatchRead {
                id,
                generation,
                entities,
                read,
            }
        });
    }

    fn toggle(&mut self, id: RequestId, entity_id: String) {
        if self.refuse_without_home_assistant(&id) {
            self.backend.audit.record(
                AUDIT_TOGGLE,
                &format!(
                    "entity={entity_id} result={}",
                    ErrorCode::HaNotConfigured.as_str()
                ),
            );
            return;
        }
        let backend = Arc::clone(&self.backend);
        self.start(detached(id.clone(), async move {
            let outcome = backend.home_assistant.toggle(&entity_id).await;
            let (reply, result) = acted(&id, outcome);
            backend
                .audit
                .record(AUDIT_TOGGLE, &format!("entity={entity_id} result={result}"));
            Finished::Acted {
                reply,
                succeeded: outcome.is_ok(),
            }
        }));
    }

    fn call(&mut self, id: RequestId, call: ServiceCall) {
        let target = format!(
            "service={}.{} entity={}",
            call.domain,
            call.service,
            call.entity_id.as_deref().unwrap_or("-")
        );
        if self.refuse_without_home_assistant(&id) {
            self.backend.audit.record(
                AUDIT_CALL,
                &format!("{target} result={}", ErrorCode::HaNotConfigured.as_str()),
            );
            return;
        }
        let backend = Arc::clone(&self.backend);
        self.start(detached(id.clone(), async move {
            let outcome = backend.home_assistant.call_service(call).await;
            let (reply, result) = acted(&id, outcome);
            backend
                .audit
                .record(AUDIT_CALL, &format!("{target} result={result}"));
            Finished::Acted {
                reply,
                succeeded: outcome.is_ok(),
            }
        }));
    }

    /// When the next regular read of the watched entities is due. None while
    /// nothing is watched, while a read is in progress, and while a `watch`
    /// request waits for its own read: its answer has to reach the client
    /// before any event about the new list.
    fn poll_due(&self) -> Option<Instant> {
        if self.watch.is_empty() || self.poll_in_progress || self.watch_reads > 0 {
            return None;
        }
        self.next_poll
    }

    fn start_poll(&mut self) {
        self.poll_in_progress = true;
        self.next_poll = None;
        let generation = self.watch.generation();
        let entities = self.watch.entities().to_vec();
        let backend = Arc::clone(&self.backend);
        self.start(async move {
            let read = backend.home_assistant.states(&entities).await;
            Finished::Polled { generation, read }
        });
    }

    /// Schedules the next read no later than `delay` from now.
    fn poll_within(&mut self, delay: std::time::Duration) {
        if self.watch.is_empty() {
            self.next_poll = None;
            return;
        }
        let at = Instant::now() + delay;
        self.next_poll = Some(self.next_poll.map_or(at, |sooner| sooner.min(at)));
    }

    /// Takes the result of finished work and returns what to send.
    fn finish(&mut self, finished: Finished) -> Vec<Message> {
        let timing = self.backend.timing;
        match finished {
            Finished::Reply(reply) => vec![reply],
            Finished::Acted { reply, succeeded } => {
                if succeeded {
                    self.poll_within(timing.after_action_poll_delay);
                }
                vec![reply]
            }
            Finished::WatchRead {
                id,
                generation,
                entities,
                read,
            } => {
                self.watch_reads -= 1;
                self.poll_within(timing.watch_poll_interval);
                match read {
                    Ok(read) => {
                        let states = states_of(&entities, &clipped(read));
                        self.watch.told(generation, &states);
                        let mut body = Message::new();
                        body.insert(
                            "states".into(),
                            Value::Object(
                                states
                                    .into_iter()
                                    .map(|(entity_id, state)| (entity_id, Value::from(state)))
                                    .collect(),
                            ),
                        );
                        vec![success(&id, body)]
                    }
                    Err(HaFailure) => vec![failure(Some(&id), ErrorCode::HaError)],
                }
            }
            Finished::Polled { generation, read } => {
                self.poll_in_progress = false;
                self.poll_within(timing.watch_poll_interval);
                // While Home Assistant cannot be reached nothing is sent: the
                // client keeps the last state it was told.
                let Ok(read) = read else {
                    return Vec::new();
                };
                self.watch
                    .changes(generation, &clipped(read))
                    .iter()
                    .map(|(entity_id, state)| state_event(entity_id, state))
                    .collect()
            }
        }
    }
}

/// Carries out a `run`, a `toggle` or a `call` as a task of its own. What it
/// sets off goes on when the connection ends, so the work, and the audit
/// record it writes at its end, must not end with the connection.
async fn detached(
    id: RequestId,
    work: impl Future<Output = Finished> + Send + 'static,
) -> Finished {
    match tokio::spawn(work).await {
        Ok(finished) => finished,
        // Only a panic in the work or the end of the runtime gets here. The
        // client learns that the request failed, as it does when a blocking
        // call of the app does not finish.
        Err(_) => Finished::Reply(failure(Some(&id), ErrorCode::HaError)),
    }
}

/// The response to a `toggle` or a `call`, and the result for the audit log.
fn acted(id: &RequestId, outcome: Result<(), HaFailure>) -> (Message, &'static str) {
    match outcome {
        Ok(()) => (success(id, Message::new()), "ok"),
        Err(HaFailure) => {
            let error = ErrorCode::HaError;
            (failure(Some(id), error), error.as_str())
        }
    }
}

fn clipped(read: HashMap<String, String>) -> HashMap<String, String> {
    read.into_iter()
        .map(|(entity_id, state)| (entity_id, entities::clip_state(&state)))
        .collect()
}
