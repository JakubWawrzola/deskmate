//! The runner: everything the service does between "started" and "stopped",
//! with no word about Windows in it. The platform layer feeds it events
//! (stop, shutdown, suspend, resume, session change, reload) and reads its
//! status; a daemon on another system drives the same runner.
//!
//! What it does, in a loop: take the seat when no Deskmate app wants the
//! connection, connect, say `info`, wait for Home Assistant's `info`, declare
//! the machine scope only to an integration that knows scoped declarations,
//! then send states, answer pings and run the four power commands. It leaves
//! as soon as an app waits for the seat and comes back when the seat is free.

use deskmate_core::config::AppConfig;
use deskmate_core::link::{
    info_frame, parse_info, ClientPlatform, ClientRole, HubInfo, LinkError, LinkFailure,
    PING_INTERVAL,
};
use deskmate_core::machine;
use deskmate_core::security::safe_preview;
use deskmate_core::sensors::OwnedSensorDef;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, MissedTickBehavior};

use crate::audit::AuditSink;
use crate::connector::{Connector, LinkSession};
use crate::power::{Clock, PowerExecutor, SafetyValve};
use crate::provisioning::{Provisioning, Setup, SetupSource};
use crate::retry::RetrySchedule;
use crate::status::{State, Status};

/// The capability an integration announces when a `declare` with a scope
/// replaces only the entities of that scope. Without it a machine-scoped
/// `declare` would delete the user's other entities.
pub const CAP_SCOPED_DECLARE: &str = "scoped_declare";
/// The capability an integration announces when it understands `bye`.
pub const CAP_BYE: &str = "bye";

/// Longest wait for a connection and its handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long Home Assistant has to answer `info`. An integration older than
/// 0.9.0 never does.
pub const INFO_TIMEOUT: Duration = Duration::from_secs(10);
/// When to look again whether the integration has been updated.
pub const TOO_OLD_RETRY: Duration = Duration::from_secs(60 * 60);
/// How often a service without the seat looks whether it is free.
const SEAT_POLL: Duration = Duration::from_secs(3);
/// The wait after handing the connection to an app, before the first look
/// at the seat: longer than the app's own poll, so the app gets there first.
const HANDOVER_PAUSE: Duration = Duration::from_secs(5);
/// A suspend that is never followed by a resume event must not keep the
/// service offline for good.
const SUSPEND_FALLBACK: Duration = Duration::from_secs(60);
/// When to read settings again that could not be used.
const INVALID_RECHECK: Duration = Duration::from_secs(60);
/// Longest wait for a goodbye and for the close of a connection.
const FAREWELL_TIMEOUT: Duration = Duration::from_secs(1);

const WAITING_FOR_APP: &str = "The Deskmate app holds the connection.";

/// The texts that name the program and how it is set up: the Windows service
/// is set up from the Deskmate app, the Linux daemon from its command line.
struct Wording {
    /// The `error` of a refused request that only an app can serve.
    unsupported: &'static str,
    not_provisioned: &'static str,
    rejected: &'static str,
    too_old: &'static str,
}

const SERVICE_WORDING: Wording = Wording {
    unsupported: "not available from the Deskmate service",
    not_provisioned: "The service has no settings yet. Set it up from the Deskmate app.",
    rejected: "Home Assistant rejected the pairing. Set the service up again from the Deskmate app.",
    too_old: "The Deskmate integration in Home Assistant is older than 0.9.0. Update it; the service looks again every hour.",
};

const DAEMON_WORDING: Wording = Wording {
    unsupported: "not supported by the Deskmate daemon",
    not_provisioned: "The daemon is not paired yet. Pair it with: sudo deskmate-service pair",
    rejected: "Home Assistant rejected the pairing. Pair the daemon again with: sudo deskmate-service pair",
    too_old: "The Deskmate integration in Home Assistant is older than 0.9.0. Update it; the daemon looks again every hour.",
};

fn wording(role: ClientRole) -> &'static Wording {
    match role {
        ClientRole::Daemon => &DAEMON_WORDING,
        ClientRole::Service | ClientRole::App => &SERVICE_WORDING,
    }
}

/// Held by whoever sent an event and wants to know when the runner has acted
/// on it. The runner drops it at that moment.
pub struct Ack(
    #[allow(dead_code, reason = "held only to be dropped")] Option<std::sync::mpsc::Sender<()>>,
);

/// The other end of an `Ack`.
pub struct AckWaiter(std::sync::mpsc::Receiver<()>);

impl Ack {
    /// For a sender that does not wait.
    pub fn none() -> Self {
        Self(None)
    }

    pub fn pair() -> (Self, AckWaiter) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (Self(Some(sender)), AckWaiter(receiver))
    }
}

impl AckWaiter {
    /// Blocks until the runner has acted on the event, at most `limit`.
    /// Returns whether it did.
    pub fn wait(self, limit: Duration) -> bool {
        // Nothing is ever sent: the channel disconnects when the Ack drops.
        matches!(
            self.0.recv_timeout(limit),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        )
    }
}

/// What the platform layer tells the runner.
pub enum Event {
    /// The service is being stopped. The connection closes without a `bye`.
    Stop,
    /// The system shuts down or restarts.
    Shutdown,
    /// The system is about to sleep or hibernate.
    Suspend(Ack),
    /// The system woke up: connect again at once.
    Resume,
    /// Someone signed in or out, or a session changed: look at the seat now.
    SessionChange,
    /// Settings or keys may have changed: read them again.
    Reload,
}

/// Decides which Deskmate process on this computer holds the connection.
pub trait Seat: Send + Sync {
    /// Whether the service may hold the connection as `node` right now.
    /// Takes a free seat; false while holding it means an app waits.
    fn may_hold(&self, node: &str) -> bool;
    /// Gives the seat up.
    fn release(&self, node: &str);
}

/// The seat of `deskmate_core::seat`, shared with the apps on this computer.
pub struct MachineSeat;

impl Seat for MachineSeat {
    fn may_hold(&self, node: &str) -> bool {
        deskmate_core::seat::machine_turn(node)
    }

    fn release(&self, node: &str) {
        deskmate_core::seat::release(node);
    }
}

/// One reading of the machine's sensors.
#[derive(Default)]
pub struct Sample {
    pub values: HashMap<String, String>,
    /// The hardware sensors detected on this computer.
    pub hardware: Vec<OwnedSensorDef>,
}

/// Reads the sensors of the machine scope. Called on a blocking thread.
pub trait Telemetry: Send + 'static {
    fn collect(&mut self, cfg: &AppConfig) -> Sample;
}

/// The collector of `deskmate_core::machine`.
pub struct MachineTelemetry(machine::Collector);

impl MachineTelemetry {
    pub fn start() -> Box<dyn Telemetry> {
        Box::new(Self(machine::Collector::new()))
    }
}

impl Telemetry for MachineTelemetry {
    fn collect(&mut self, cfg: &AppConfig) -> Sample {
        let (values, hardware) = self.0.collect(cfg);
        Sample { values, hardware }
    }
}

/// Starts a collector. Called on a blocking thread, again after a collector
/// failed.
pub type TelemetryFactory = Arc<dyn Fn() -> Box<dyn Telemetry> + Send + Sync>;

/// Everything the runner needs from outside.
pub struct Parts<C: Connector> {
    pub connector: C,
    pub seat: Arc<dyn Seat>,
    pub clock: Arc<dyn Clock>,
    pub power: Arc<dyn PowerExecutor>,
    pub audit: Arc<dyn AuditSink>,
    pub source: Arc<dyn SetupSource>,
    pub telemetry: TelemetryFactory,
    pub role: ClientRole,
    pub platform: ClientPlatform,
}

/// Where the platform layer puts its events.
pub type EventSender = mpsc::UnboundedSender<Event>;
/// Where the runner takes them from.
pub type EventReceiver = mpsc::UnboundedReceiver<Event>;

/// The channel for events. It exists before the runner does, so that a stop
/// that arrives while the service is still starting is not lost.
pub fn event_channel() -> (EventSender, EventReceiver) {
    mpsc::unbounded_channel()
}

pub struct Runner<C: Connector> {
    connector: Arc<C>,
    seat: Arc<dyn Seat>,
    power: Arc<dyn PowerExecutor>,
    audit: Arc<dyn AuditSink>,
    source: Arc<dyn SetupSource>,
    new_telemetry: TelemetryFactory,
    telemetry: Option<Box<dyn Telemetry>>,
    /// The last reading of the sensors.
    reading: Option<Sample>,
    role: ClientRole,
    platform: ClientPlatform,
    valve: SafetyValve,
    events: mpsc::UnboundedReceiver<Event>,
    status: watch::Sender<Status>,
}

/// Why the runner stops serving one setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Stop,
    /// The settings changed: start over with what is stored now.
    Reload,
}

/// How one session ended.
enum Outcome {
    Exit(Exit),
    /// An app waits for the seat. The connection is closed.
    Handover,
    /// The system goes to sleep. The connection is closed.
    Suspended,
    /// The integration does not know scoped declarations.
    TooOld,
    Failed(LinkError),
}

/// How a wait ended.
enum Wake {
    Elapsed,
    /// The system woke up: no more waiting.
    Resumed,
    Exit(Exit),
}

enum Pause {
    For(Duration),
    /// Like `For(SEAT_POLL)`, and a session change ends it at once.
    SeatPoll,
    /// Until the system wakes up, or `SUSPEND_FALLBACK` has passed.
    Suspended,
}

/// What a frame from Home Assistant leads to.
enum Reaction {
    Nothing,
    Reply(Value),
    Hub(HubInfo),
}

/// The `bye` frame: why this client is about to close the connection.
pub fn bye_frame(reason: &str) -> Value {
    json!({"t": "bye", "reason": reason})
}

fn ack_frame(id: Value, result: Result<(), String>) -> Value {
    let mut ack = json!({"t": "ack", "id": id, "ok": result.is_ok()});
    if let Err(error) = result {
        ack["error"] = json!(error);
    }
    ack
}

/// The status of a client of this role that has no settings.
fn not_provisioned(role: ClientRole) -> Status {
    Status {
        detail: wording(role).not_provisioned.into(),
        ..Status::not_provisioned()
    }
}

fn frame_id(frame: &Value) -> Value {
    frame.get("id").cloned().unwrap_or(Value::Null)
}

impl<C: Connector> Runner<C> {
    /// A runner that takes its events from `events`, and where to read its
    /// status.
    pub fn new(parts: Parts<C>, events: EventReceiver) -> (Self, watch::Receiver<Status>) {
        let (status, status_rx) = watch::channel(not_provisioned(parts.role));
        let runner = Self {
            connector: Arc::new(parts.connector),
            seat: parts.seat,
            power: parts.power,
            audit: parts.audit,
            source: parts.source,
            new_telemetry: parts.telemetry,
            telemetry: None,
            reading: None,
            role: parts.role,
            platform: parts.platform,
            valve: SafetyValve::new(parts.clock),
            events,
            status,
        };
        (runner, status_rx)
    }

    /// Runs until a stop or a shutdown event, or until every sender of
    /// events is gone.
    pub async fn run(mut self) {
        log::info!("runner started");
        loop {
            let exit = match self.source.load() {
                Provisioning::NotProvisioned => {
                    self.set_status(not_provisioned(self.role));
                    self.idle(None).await
                }
                Provisioning::Invalid(detail) => {
                    self.set_status(Status {
                        state: State::Error,
                        node_id: String::new(),
                        detail,
                        fingerprint: String::new(),
                    });
                    self.idle(Some(INVALID_RECHECK)).await
                }
                Provisioning::Ready(setup) => self.serve(&setup).await,
            };
            if exit == Exit::Stop {
                break;
            }
        }
        log::info!("runner stopped");
    }

    fn set_status(&self, status: Status) {
        self.status.send_if_modified(|current| {
            if *current == status {
                return false;
            }
            log::info!("state: {} ({})", status.state.as_str(), status.detail);
            *current = status;
            true
        });
    }

    fn state(&self) -> State {
        self.status.borrow().state
    }

    fn report(&self, setup: &Setup, state: State, detail: impl Into<String>) {
        self.set_status(Status {
            state,
            node_id: setup.config.node_id.clone(),
            detail: detail.into(),
            fingerprint: setup.fingerprint.clone(),
        });
    }

    /// Waits without settings that can be used: until they may have changed
    /// (a reload, or `recheck` has passed) or the service stops.
    async fn idle(&mut self, recheck: Option<Duration>) -> Exit {
        let timed = recheck.is_some();
        let recheck = tokio::time::sleep(recheck.unwrap_or_default());
        tokio::pin!(recheck);
        loop {
            tokio::select! {
                _ = &mut recheck, if timed => return Exit::Reload,
                event = self.events.recv() => match event {
                    None | Some(Event::Stop | Event::Shutdown) => return Exit::Stop,
                    Some(Event::Reload) => return Exit::Reload,
                    Some(Event::Resume) => self.valve.note_wake(),
                    // An Ack in a suspend event is dropped here: nothing is
                    // open that would have to be closed first.
                    Some(Event::Suspend(_) | Event::SessionChange) => {}
                },
            }
        }
    }

    /// Whether what is stored now differs from the setup in use.
    fn setup_changed(&self, current: &Setup) -> bool {
        match self.source.load() {
            Provisioning::Ready(stored) => stored.fingerprint != current.fingerprint,
            _ => true,
        }
    }

    /// Connects, reconnects and hands over for one setup.
    async fn serve(&mut self, setup: &Setup) -> Exit {
        let cfg = setup.config.to_app_config();
        let node = setup.config.node_id.as_str();
        let mut retry = RetrySchedule::new(setup.config.endpoints());
        // The failure that was logged last. While attempts keep failing the
        // same way, neither the log nor the status repeats it.
        let mut logged: Option<(LinkFailure, String)> = None;
        self.report(setup, State::Connecting, "Connecting to Home Assistant.");
        let exit = loop {
            match self.wait_for_seat(setup).await {
                Wake::Exit(exit) => break exit,
                Wake::Resumed => retry.reset(),
                Wake::Elapsed => {}
            }
            // A failure stays on show until an attempt ends differently.
            if !matches!(self.state(), State::Connecting | State::Error) {
                self.report(setup, State::Connecting, "Connecting to Home Assistant.");
            }
            let url = retry.endpoint().to_string();
            let outcome = self.session(setup, &cfg, &url).await;
            if !matches!(outcome, Outcome::Failed(_)) {
                logged = None;
            }
            let pause = match outcome {
                Outcome::Exit(exit) => break exit,
                Outcome::Handover => {
                    retry.reset();
                    log::info!("an app waits for the connection: handed over");
                    self.report(setup, State::Waiting, WAITING_FOR_APP);
                    Pause::For(HANDOVER_PAUSE)
                }
                Outcome::Suspended => {
                    retry.reset();
                    self.report(setup, State::Waiting, "The computer is going to sleep.");
                    Pause::Suspended
                }
                Outcome::TooOld => {
                    retry.reset();
                    // Nothing is connected for the next hour, so an app may
                    // as well have the seat.
                    self.seat.release(node);
                    self.report(setup, State::IntegrationTooOld, wording(self.role).too_old);
                    Pause::For(TOO_OLD_RETRY)
                }
                Outcome::Failed(error) => {
                    let label = retry.label();
                    let delay = retry.on_failure(error.kind);
                    let (state, detail) = match error.kind {
                        LinkFailure::Auth => (State::Error, wording(self.role).rejected),
                        LinkFailure::Locked => (
                            State::Error,
                            "Home Assistant is refusing this computer after repeated failed attempts. That clears by itself within five minutes.",
                        ),
                        LinkFailure::Clock => (
                            State::Error,
                            "This computer's clock and Home Assistant's differ by more than 90 seconds.",
                        ),
                        LinkFailure::Transport => (
                            State::Connecting,
                            "Home Assistant cannot be reached. Trying again.",
                        ),
                    };
                    let failure = (error.kind, error.message);
                    if logged.as_ref() != Some(&failure) {
                        log::warn!(
                            "no session{label} ({:?}): {}; trying again",
                            failure.0,
                            safe_preview(&failure.1, 200)
                        );
                        logged = Some(failure);
                    }
                    self.report(setup, state, detail);
                    Pause::For(delay)
                }
            };
            match self.pause(setup, pause).await {
                Wake::Exit(exit) => break exit,
                Wake::Resumed => retry.reset(),
                Wake::Elapsed => {}
            }
        };
        self.seat.release(node);
        exit
    }

    /// Waits until this service may hold the connection.
    async fn wait_for_seat(&mut self, setup: &Setup) -> Wake {
        let mut resumed = false;
        loop {
            if self.seat.may_hold(&setup.config.node_id) {
                return if resumed {
                    Wake::Resumed
                } else {
                    Wake::Elapsed
                };
            }
            // A false while holding the seat keeps it (`machine_turn`). After
            // a failed attempt or a suspend nothing is open here, and nothing
            // else would give the seat back to the app that waits for it.
            self.seat.release(&setup.config.node_id);
            self.report(setup, State::Waiting, WAITING_FOR_APP);
            match self.pause(setup, Pause::SeatPoll).await {
                Wake::Exit(exit) => return Wake::Exit(exit),
                Wake::Resumed => resumed = true,
                Wake::Elapsed => {}
            }
        }
    }

    /// Waits while nothing is connected, and acts on the events that arrive.
    async fn pause(&mut self, setup: &Setup, pause: Pause) -> Wake {
        let (duration, seat_poll, mut suspended) = match pause {
            Pause::For(duration) => (duration, false, false),
            Pause::SeatPoll => (SEAT_POLL, true, false),
            Pause::Suspended => (SUSPEND_FALLBACK, false, true),
        };
        let sleep = tokio::time::sleep(duration);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => {
                    if suspended {
                        log::warn!("no resume event after a suspend; carrying on");
                    }
                    return Wake::Elapsed;
                }
                event = self.events.recv() => match event {
                    None | Some(Event::Stop | Event::Shutdown) => return Wake::Exit(Exit::Stop),
                    Some(Event::Reload) => {
                        if self.setup_changed(setup) {
                            return Wake::Exit(Exit::Reload);
                        }
                    }
                    Some(Event::Suspend(ack)) => {
                        // Nothing is open; the sender need not wait.
                        drop(ack);
                        if !suspended {
                            suspended = true;
                            self.report(setup, State::Waiting, "The computer is going to sleep.");
                            sleep.as_mut().reset(Instant::now() + SUSPEND_FALLBACK);
                        }
                    }
                    Some(Event::Resume) => {
                        self.valve.note_wake();
                        return Wake::Resumed;
                    }
                    Some(Event::SessionChange) => {
                        if seat_poll {
                            return Wake::Elapsed;
                        }
                    }
                },
            }
        }
    }

    /// What an event means while a connection is being opened. None: carry
    /// on connecting.
    fn event_while_connecting(&mut self, setup: &Setup, event: Option<Event>) -> Option<Outcome> {
        match event {
            None | Some(Event::Stop | Event::Shutdown) => Some(Outcome::Exit(Exit::Stop)),
            Some(Event::Suspend(_)) => Some(Outcome::Suspended),
            Some(Event::Resume) => {
                self.valve.note_wake();
                None
            }
            Some(Event::SessionChange) => {
                if self.seat.may_hold(&setup.config.node_id) {
                    None
                } else {
                    self.seat.release(&setup.config.node_id);
                    Some(Outcome::Handover)
                }
            }
            Some(Event::Reload) => self
                .setup_changed(setup)
                .then_some(Outcome::Exit(Exit::Reload)),
        }
    }

    /// One connection attempt and, when it succeeds, one session.
    async fn session(&mut self, setup: &Setup, cfg: &AppConfig, url: &str) -> Outcome {
        let node = setup.config.node_id.as_str();
        // Read the sensors now, so the session can declare the moment Home
        // Assistant has said what it is.
        if let Some(sample) = self.sample(cfg).await {
            self.reading = Some(sample);
        }
        let connector = Arc::clone(&self.connector);
        let attempt =
            tokio::time::timeout(CONNECT_TIMEOUT, connector.connect(url, node, &setup.keys));
        tokio::pin!(attempt);
        let mut link = loop {
            tokio::select! {
                result = &mut attempt => match result {
                    Ok(Ok(link)) => break link,
                    Ok(Err(error)) => return Outcome::Failed(error),
                    Err(_) => return Outcome::Failed("the connection attempt timed out".into()),
                },
                event = self.events.recv() => {
                    if let Some(outcome) = self.event_while_connecting(setup, event) {
                        return outcome;
                    }
                }
            }
        };
        let (outcome, ack) = self.converse(&mut link, setup, cfg).await;
        let _ = tokio::time::timeout(FAREWELL_TIMEOUT, link.close()).await;
        // Whoever announced a suspend may let the system sleep now.
        drop(ack);
        if matches!(outcome, Outcome::Handover) {
            // Only now: the app must not connect while this session is open.
            self.seat.release(node);
        }
        outcome
    }

    /// Tells Home Assistant why the connection is about to close, when it
    /// understands that.
    async fn farewell<S: LinkSession>(link: &mut S, hub: Option<&HubInfo>, reason: &str) {
        if hub.is_some_and(|hub| hub.has_cap(CAP_BYE)) {
            let _ = tokio::time::timeout(FAREWELL_TIMEOUT, link.send(&bye_frame(reason))).await;
        }
    }

    /// The session on an open connection, from `info` to its end. The caller
    /// closes the connection and then drops the returned Ack.
    async fn converse<S: LinkSession>(
        &mut self,
        link: &mut S,
        setup: &Setup,
        cfg: &AppConfig,
    ) -> (Outcome, Option<Ack>) {
        let node = setup.config.node_id.as_str();
        if let Err(error) = link.send(&info_frame(self.role, self.platform, &[])).await {
            return (Outcome::Failed(error), None);
        }
        // None until Home Assistant has announced scoped declarations.
        let mut hub: Option<HubInfo> = None;
        let mut declared: Vec<OwnedSensorDef> = Vec::new();

        let start = Instant::now();
        let info_deadline = tokio::time::sleep_until(start + INFO_TIMEOUT);
        tokio::pin!(info_deadline);
        let period = Duration::from_secs(cfg.publish_interval_secs.clamp(2, 3600));
        let mut state_timer = tokio::time::interval_at(start + period, period);
        let mut ping = tokio::time::interval_at(start + PING_INTERVAL, PING_INTERVAL);
        let check_every = deskmate_core::seat::CHECK_EVERY;
        let mut seat_check = tokio::time::interval_at(start + check_every, check_every);
        for timer in [&mut state_timer, &mut ping, &mut seat_check] {
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
        }

        loop {
            let mut look_at_seat = false;
            tokio::select! {
                _ = &mut info_deadline, if hub.is_none() => {
                    log::warn!("Home Assistant did not answer info");
                    return (Outcome::TooOld, None);
                }
                _ = seat_check.tick() => look_at_seat = true,
                _ = ping.tick() => {
                    if let Err(error) = link.send(&json!({"t": "ping"})).await {
                        return (Outcome::Failed(error), None);
                    }
                }
                _ = state_timer.tick(), if hub.is_some() => {
                    if let Err(error) = self.send_states(link, cfg, &mut declared, false).await {
                        return (Outcome::Failed(error), None);
                    }
                }
                event = self.events.recv() => match event {
                    None | Some(Event::Stop) => return (Outcome::Exit(Exit::Stop), None),
                    Some(Event::Shutdown) => {
                        Self::farewell(link, hub.as_ref(), "shutdown").await;
                        return (Outcome::Exit(Exit::Stop), None);
                    }
                    Some(Event::Suspend(ack)) => {
                        Self::farewell(link, hub.as_ref(), "suspend").await;
                        return (Outcome::Suspended, Some(ack));
                    }
                    Some(Event::Resume) => {
                        // A suspend the service was not told about: find out
                        // now whether the connection survived it.
                        self.valve.note_wake();
                        if let Err(error) = link.send(&json!({"t": "ping"})).await {
                            return (Outcome::Failed(error), None);
                        }
                    }
                    Some(Event::SessionChange) => look_at_seat = true,
                    Some(Event::Reload) => {
                        if self.setup_changed(setup) {
                            return (Outcome::Exit(Exit::Reload), None);
                        }
                    }
                },
                inbound = link.receive() => {
                    let frame = match inbound {
                        Ok(Some(frame)) => frame,
                        Ok(None) => {
                            return (
                                Outcome::Failed("Home Assistant closed the connection".into()),
                                None,
                            )
                        }
                        Err(error) => return (Outcome::Failed(error), None),
                    };
                    match self.react(&frame, hub.is_some()) {
                        Reaction::Nothing => {}
                        Reaction::Reply(reply) => {
                            if let Err(error) = link.send(&reply).await {
                                return (Outcome::Failed(error), None);
                            }
                        }
                        Reaction::Hub(_) if hub.is_some() => {}
                        Reaction::Hub(info) => {
                            if !info.has_cap(CAP_SCOPED_DECLARE) {
                                log::warn!(
                                    "the integration {} does not know scoped declarations",
                                    info.integration
                                );
                                return (Outcome::TooOld, None);
                            }
                            log::info!("connected to the integration {}", info.integration);
                            hub = Some(info);
                            if let Err(error) =
                                self.send_states(link, cfg, &mut declared, true).await
                            {
                                return (Outcome::Failed(error), None);
                            }
                            state_timer.reset();
                            self.report(setup, State::Connected, "Connected to Home Assistant.");
                        }
                    }
                }
            }
            if look_at_seat && !self.seat.may_hold(node) {
                Self::farewell(link, hub.as_ref(), "handover").await;
                return (Outcome::Handover, None);
            }
        }
    }

    /// Reads the sensors on a blocking thread. None when the collector
    /// failed; a new one starts with the next reading.
    async fn sample(&mut self, cfg: &AppConfig) -> Option<Sample> {
        let existing = self.telemetry.take();
        let factory = Arc::clone(&self.new_telemetry);
        let cfg = cfg.clone();
        let reading = tokio::task::spawn_blocking(move || {
            let mut telemetry = existing.unwrap_or_else(|| factory());
            let sample = telemetry.collect(&cfg);
            (telemetry, sample)
        })
        .await;
        match reading {
            Ok((telemetry, sample)) => {
                self.telemetry = Some(telemetry);
                Some(sample)
            }
            Err(_) => {
                log::error!("the sensor collector failed; a new one starts with the next reading");
                None
            }
        }
    }

    /// Sends the current states. A `declare` goes first when this is the
    /// first time in the session or the detected hardware changed, as in the
    /// app. The first time uses the reading taken before connecting: Home
    /// Assistant shows the computer as available with the first `declare`,
    /// which must not wait for the sensors.
    async fn send_states<S: LinkSession>(
        &mut self,
        link: &mut S,
        cfg: &AppConfig,
        declared: &mut Vec<OwnedSensorDef>,
        first: bool,
    ) -> Result<(), LinkError> {
        if !first {
            match self.sample(cfg).await {
                Some(sample) => self.reading = Some(sample),
                None => return Ok(()),
            }
        }
        let hardware = self.reading.as_ref().map(|reading| &reading.hardware);
        if first || hardware.is_some_and(|hardware| hardware != declared) {
            if let Some(hardware) = hardware {
                declared.clone_from(hardware);
            }
            link.send(&machine::declare_frame(cfg, declared)).await?;
        }
        match &self.reading {
            Some(reading) if !reading.values.is_empty() => {
                link.send(&machine::state_frame(&reading.values)).await
            }
            _ => Ok(()),
        }
    }

    /// Decides what a frame from Home Assistant leads to. The frame itself
    /// is never logged.
    fn react(&mut self, frame: &Value, ready: bool) -> Reaction {
        match frame.get("t").and_then(Value::as_str) {
            Some("ping") => Reaction::Reply(json!({"t": "pong"})),
            Some("pong") => Reaction::Nothing,
            Some("info") => match parse_info(frame) {
                Some(info) => Reaction::Hub(info),
                None => {
                    log::warn!("ignoring an invalid info frame from Home Assistant");
                    Reaction::Nothing
                }
            },
            Some("cmd") => {
                let key = frame.get("key").and_then(Value::as_str).unwrap_or("");
                let action = frame.get("action").and_then(Value::as_str).unwrap_or("set");
                let result = self.run_command(key, action, ready);
                Reaction::Reply(ack_frame(frame_id(frame), result))
            }
            // Requests that only an app in a user's session can serve. Home
            // Assistant gets an answer at once and does not wait for a timeout.
            Some("notify") => Reaction::Reply(ack_frame(
                frame_id(frame),
                Err(wording(self.role).unsupported.into()),
            )),
            Some(kind @ ("fs" | "tv")) => Reaction::Reply(json!({
                "t": format!("{kind}_res"),
                "id": frame_id(frame),
                "ok": false,
                "error": wording(self.role).unsupported,
            })),
            _ => {
                log::warn!("ignoring a frame of an unknown type");
                Reaction::Nothing
            }
        }
    }

    /// Runs one of the four power commands, or says why not. Every decision
    /// goes into the audit log; a key that is not one of the four does not.
    fn run_command(&mut self, key: &str, action: &str, ready: bool) -> Result<(), String> {
        // From here on `key` is a name from this program's own list.
        let Some(key) = machine::COMMAND_KEYS
            .into_iter()
            .find(|known| *known == key)
        else {
            self.audit
                .record("power_command", "result=refused_not_a_power_command");
            return Err(wording(self.role).unsupported.into());
        };
        if action != "press" {
            self.audit
                .record("power_command", &format!("key={key} result=refused_action"));
            return Err("invalid command envelope".into());
        }
        if !ready {
            self.audit.record(
                "power_command",
                &format!("key={key} result=refused_not_ready"),
            );
            return Err("the service is not ready yet".into());
        }
        if let Err(remaining) = self.valve.check() {
            self.audit.record(
                "power_command",
                &format!("key={key} result=refused_after_start"),
            );
            log::warn!("power command {key} refused: the computer started or woke less than five minutes ago");
            return Err(format!(
                "refused: power commands stay off for five minutes after the computer starts or wakes ({} s left)",
                remaining.as_secs()
            ));
        }
        let executor = self.power.name();
        match self.power.run(key) {
            Ok(()) => {
                self.audit.record(
                    "power_command",
                    &format!("key={key} executor={executor} result=ok"),
                );
                log::info!("power command {key} ({executor})");
                Ok(())
            }
            Err(error) => {
                self.audit.record(
                    "power_command",
                    &format!("key={key} executor={executor} result=error"),
                );
                log::error!("power command {key} failed: {}", safe_preview(&error, 200));
                Err("the command could not be started".into())
            }
        }
    }
}

#[cfg(test)]
mod tests;
