//! The runner against a stand-in for Home Assistant, a seat, a clock and a
//! power executor the tests control. Time is tokio's paused clock: a wait in
//! the runner costs no real time, and how long it took can be measured.

use super::*;
use crate::config::ServiceConfig;
use crate::power::tests::FakeClock;
use crate::secrets::Keys;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// base64 of 32 bytes of 0x01
const KEY: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const LOCAL: &str = "ws://192.168.1.10:8123/api/deskmate_link/ws";
const REMOTE: &str = "wss://example.com/api/deskmate_link/ws";
const ALL_CAPS: [&str; 3] = ["info", "scoped_declare", "bye"];
/// Longer than any wait in the runner: reaching it means nothing came.
const NOTHING_CAME: Duration = Duration::from_secs(3 * 60 * 60);
const UNSUPPORTED: &str = SERVICE_WORDING.unsupported;

/// What the stand-in hub and the seat saw, in order.
#[derive(Debug, PartialEq)]
enum Seen {
    Refused {
        url: String,
    },
    Connected {
        session: usize,
        url: String,
        node: String,
    },
    Frame {
        session: usize,
        frame: Value,
    },
    Closed {
        session: usize,
    },
    SeatReleased,
}

struct Hub {
    seen: mpsc::UnboundedSender<Seen>,
    /// The next connection attempts fail with these.
    refusals: Mutex<VecDeque<LinkError>>,
    /// What the hub answers to `info` by itself; None: it stays silent.
    info_answer: Mutex<Option<Value>>,
    /// One sender per session for frames to the client; None closes.
    to_client: Mutex<Vec<mpsc::UnboundedSender<Option<Value>>>>,
}

struct FakeConnector(Arc<Hub>);

struct FakeSession {
    number: usize,
    hub: Arc<Hub>,
    inbound: mpsc::UnboundedReceiver<Option<Value>>,
    answers: mpsc::UnboundedSender<Option<Value>>,
    closed: bool,
}

impl Connector for FakeConnector {
    type Session = FakeSession;

    async fn connect(&self, url: &str, node: &str, _keys: &Keys) -> Result<FakeSession, LinkError> {
        if let Some(error) = self.0.refusals.lock().unwrap().pop_front() {
            let _ = self.0.seen.send(Seen::Refused { url: url.into() });
            return Err(error);
        }
        let (answers, inbound) = mpsc::unbounded_channel();
        let number = {
            let mut sessions = self.0.to_client.lock().unwrap();
            sessions.push(answers.clone());
            sessions.len()
        };
        let _ = self.0.seen.send(Seen::Connected {
            session: number,
            url: url.into(),
            node: node.into(),
        });
        Ok(FakeSession {
            number,
            hub: Arc::clone(&self.0),
            inbound,
            answers,
            closed: false,
        })
    }
}

impl LinkSession for FakeSession {
    async fn send(&mut self, frame: &Value) -> Result<(), LinkError> {
        if self.closed {
            return Err("the session is closed".into());
        }
        let _ = self.hub.seen.send(Seen::Frame {
            session: self.number,
            frame: frame.clone(),
        });
        if frame["t"] == "info" {
            let answer = self.hub.info_answer.lock().unwrap().clone();
            if let Some(answer) = answer {
                let _ = self.answers.send(Some(answer));
            }
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<Option<Value>, LinkError> {
        Ok(self.inbound.recv().await.flatten())
    }

    async fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            let _ = self.hub.seen.send(Seen::Closed {
                session: self.number,
            });
        }
    }
}

struct FakeSeat {
    seen: mpsc::UnboundedSender<Seen>,
    /// An app waits for the seat, and holds it once the service has let go.
    app_wants_it: AtomicBool,
    held: AtomicBool,
}

impl FakeSeat {
    /// Like the real seat: an app that waits gets it only after the service
    /// released it.
    fn app_has_it(&self) -> bool {
        self.app_wants_it.load(Ordering::SeqCst) && !self.held.load(Ordering::SeqCst)
    }
}

impl Seat for FakeSeat {
    fn may_hold(&self, _node: &str) -> bool {
        if self.app_wants_it.load(Ordering::SeqCst) {
            // While held, the seat stays held until `release`.
            return false;
        }
        self.held.store(true, Ordering::SeqCst);
        true
    }

    fn release(&self, _node: &str) {
        if self.held.swap(false, Ordering::SeqCst) {
            let _ = self.seen.send(Seen::SeatReleased);
        }
    }
}

#[derive(Default)]
struct FakePower {
    ran: Mutex<Vec<String>>,
    fails: AtomicBool,
}

impl PowerExecutor for FakePower {
    fn run(&self, key: &str) -> Result<(), String> {
        if self.fails.load(Ordering::SeqCst) {
            return Err("the system said no".into());
        }
        self.ran.lock().unwrap().push(key.into());
        Ok(())
    }

    fn name(&self) -> &'static str {
        "fake"
    }
}

#[derive(Default)]
struct MemoryAudit(Mutex<Vec<String>>);

impl AuditSink for MemoryAudit {
    fn record(&self, event: &str, result: &str) {
        self.0.lock().unwrap().push(format!("{event} {result}"));
    }
}

/// The stored setup the tests change by hand.
enum Stored {
    Nothing,
    Broken(&'static str),
    Config(ServiceConfig),
}

struct FakeSource(Mutex<Stored>);

impl SetupSource for FakeSource {
    fn load(&self) -> Provisioning {
        match &*self.0.lock().unwrap() {
            Stored::Nothing => Provisioning::NotProvisioned,
            Stored::Broken(detail) => Provisioning::Invalid((*detail).into()),
            Stored::Config(config) => {
                let keys = Keys::decode(KEY, None).unwrap();
                let fingerprint = config.fingerprint(&keys.pairing, None).unwrap();
                Provisioning::Ready(Box::new(Setup {
                    config: config.clone(),
                    keys,
                    fingerprint,
                }))
            }
        }
    }
}

struct Readings {
    values: HashMap<String, String>,
    hardware: Vec<OwnedSensorDef>,
    offered: machine::Offered,
    /// The threads `offered` was asked on.
    offered_on: Vec<std::thread::ThreadId>,
}

struct FakeTelemetry(Arc<Mutex<Readings>>);

impl Telemetry for FakeTelemetry {
    fn collect(&mut self, _cfg: &AppConfig) -> Sample {
        let readings = self.0.lock().unwrap();
        Sample {
            values: readings.values.clone(),
            hardware: readings.hardware.clone(),
        }
    }

    fn offered(&mut self) -> machine::Offered {
        let mut readings = self.0.lock().unwrap();
        readings.offered_on.push(std::thread::current().id());
        readings.offered
    }
}

fn config(node: &str, remote: bool) -> ServiceConfig {
    ServiceConfig {
        node_id: node.into(),
        link_url: LOCAL.into(),
        link_url_remote: if remote { REMOTE.into() } else { String::new() },
        link_cascade: false,
        publish_interval_secs: 15,
        device_name: "Test PC".into(),
        sensors_enabled: Default::default(),
    }
}

fn hub_info(caps: &[&str]) -> Value {
    json!({"t": "info", "integration": "0.9.0", "caps": caps})
}

fn gpu_sensor() -> OwnedSensorDef {
    OwnedSensorDef {
        id: "gpu_usage".into(),
        name: "GPU usage".into(),
        component: "sensor".into(),
        unit: Some("%".into()),
        device_class: None,
        icon: None,
        privacy: false,
        default_enabled: true,
        hardware: true,
    }
}

fn transport() -> LinkError {
    LinkError::from("no route")
}

fn refusal(kind: LinkFailure) -> LinkError {
    LinkError::new(kind, "refused")
}

struct Rig {
    hub: Arc<Hub>,
    seen: mpsc::UnboundedReceiver<Seen>,
    events: mpsc::UnboundedSender<Event>,
    status: watch::Receiver<Status>,
    seat: Arc<FakeSeat>,
    clock: Arc<FakeClock>,
    power: Arc<FakePower>,
    audit: Arc<MemoryAudit>,
    source: Arc<FakeSource>,
    readings: Arc<Mutex<Readings>>,
    runner: tokio::task::JoinHandle<()>,
}

impl Rig {
    /// Starts a runner. With `answer` the hub answers `info` by itself.
    fn start(stored: Stored, answer: Option<Value>) -> Self {
        Self::start_with(stored, answer, |_| {})
    }

    /// Like `start`; `prepare` runs before the runner does.
    fn start_with(stored: Stored, answer: Option<Value>, prepare: impl FnOnce(&Rig)) -> Self {
        Self::start_as(
            ClientRole::Service,
            ClientPlatform::Windows,
            stored,
            answer,
            prepare,
        )
    }

    /// Like `start_with`, for a client of this role on this platform.
    fn start_as(
        role: ClientRole,
        platform: ClientPlatform,
        stored: Stored,
        answer: Option<Value>,
        prepare: impl FnOnce(&Rig),
    ) -> Self {
        let (seen_tx, seen) = mpsc::unbounded_channel();
        let hub = Arc::new(Hub {
            seen: seen_tx.clone(),
            refusals: Mutex::default(),
            info_answer: Mutex::new(answer),
            to_client: Mutex::default(),
        });
        let seat = Arc::new(FakeSeat {
            seen: seen_tx,
            app_wants_it: AtomicBool::new(false),
            held: AtomicBool::new(false),
        });
        // Long after boot: the valve is open unless a test says otherwise.
        let clock = FakeClock::at(86_400);
        let power = Arc::new(FakePower::default());
        let audit = Arc::new(MemoryAudit::default());
        let source = Arc::new(FakeSource(Mutex::new(stored)));
        let readings = Arc::new(Mutex::new(Readings {
            values: HashMap::from([("cpu".to_string(), "12.5".to_string())]),
            hardware: Vec::new(),
            offered: machine::Offered::ALL,
            offered_on: Vec::new(),
        }));
        let for_factory = Arc::clone(&readings);
        let (events, events_rx) = event_channel();
        let (runner, status) = Runner::new(
            Parts {
                connector: FakeConnector(Arc::clone(&hub)),
                seat: seat.clone(),
                clock: clock.clone(),
                power: power.clone(),
                audit: audit.clone(),
                source: source.clone(),
                telemetry: Arc::new(move || {
                    Box::new(FakeTelemetry(Arc::clone(&for_factory))) as Box<dyn Telemetry>
                }),
                role,
                platform,
            },
            events_rx,
        );
        let mut rig = Self {
            hub,
            seen,
            events,
            status,
            seat,
            clock,
            power,
            audit,
            source,
            readings,
            runner: tokio::spawn(async {}),
        };
        prepare(&rig);
        rig.runner = tokio::spawn(runner.run());
        rig
    }

    /// A runner with a connected session in which the hub announced every
    /// capability; the first `declare` and `state` have been read.
    async fn connected() -> (Self, usize) {
        Self::connected_to(hub_info(&ALL_CAPS)).await
    }

    async fn connected_to(answer: Value) -> (Self, usize) {
        let mut rig = Self::start(Stored::Config(config("test_pc", false)), Some(answer));
        let session = rig.session_opens().await;
        rig.frame("info").await;
        rig.frame("declare").await;
        rig.frame("state").await;
        assert_eq!(rig.state(), State::Connected);
        (rig, session)
    }

    async fn next(&mut self) -> Seen {
        match tokio::time::timeout(NOTHING_CAME, self.seen.recv()).await {
            Ok(Some(seen)) => seen,
            _ => panic!("nothing happened"),
        }
    }

    async fn session_opens(&mut self) -> usize {
        match self.next().await {
            Seen::Connected { session, .. } => session,
            other => panic!("expected a new session, got {other:?}"),
        }
    }

    async fn session_closes(&mut self, session: usize) {
        assert_eq!(self.next().await, Seen::Closed { session });
    }

    /// The next frame from the service, which must be of this type.
    async fn frame(&mut self, kind: &str) -> Value {
        match self.next().await {
            Seen::Frame { frame, .. } if frame["t"] == kind => frame,
            other => panic!("expected a {kind} frame, got {other:?}"),
        }
    }

    /// The next frame of this type, leaving out the frames a session sends
    /// on its own schedule.
    async fn frame_after_routine(&mut self, kind: &str) -> Value {
        loop {
            match self.next().await {
                Seen::Frame { frame, .. } if frame["t"] == kind => return frame,
                Seen::Frame { frame, .. } if frame["t"] == "ping" || frame["t"] == "state" => {}
                other => panic!("expected a {kind} frame, got {other:?}"),
            }
        }
    }

    /// Lets everything that is ready run, then checks that nothing was seen.
    async fn nothing_more(&mut self) {
        tokio::time::sleep(Duration::from_millis(1)).await;
        if let Ok(seen) = self.seen.try_recv() {
            panic!("expected nothing, got {seen:?}");
        }
    }

    /// Nothing is seen for this long. Only for a runner without a session.
    async fn nothing_for(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
        if let Ok(seen) = self.seen.try_recv() {
            panic!("expected nothing, got {seen:?}");
        }
    }

    fn hub_sends(&self, session: usize, frame: Value) {
        self.hub.to_client.lock().unwrap()[session - 1]
            .send(Some(frame))
            .unwrap();
    }

    fn hub_closes(&self, session: usize) {
        self.hub.to_client.lock().unwrap()[session - 1]
            .send(None)
            .unwrap();
    }

    fn event(&self, event: Event) {
        self.events.send(event).ok().unwrap();
    }

    fn state(&self) -> State {
        self.status.borrow().state
    }

    async fn state_becomes(&mut self, state: State) -> Status {
        let reached = self.status.wait_for(|status| status.state == state);
        match tokio::time::timeout(NOTHING_CAME, reached).await {
            Ok(Ok(status)) => status.clone(),
            _ => panic!("the state never became {state:?}"),
        }
    }

    fn store(&self, stored: Stored) {
        *self.source.0.lock().unwrap() = stored;
    }

    fn audit_lines(&self) -> Vec<String> {
        self.audit.0.lock().unwrap().clone()
    }

    fn commands_run(&self) -> Vec<String> {
        self.power.ran.lock().unwrap().clone()
    }

    async fn command(&mut self, session: usize, id: u64, key: &str, action: &str) -> Value {
        self.hub_sends(
            session,
            json!({"t": "cmd", "id": id, "key": key, "action": action}),
        );
        let ack = self.frame_after_routine("ack").await;
        assert_eq!(ack["id"], id);
        ack
    }

    /// Waits for the runner to end.
    async fn runner_ends(&mut self) {
        tokio::time::timeout(NOTHING_CAME, &mut self.runner)
            .await
            .expect("the runner must end")
            .unwrap();
    }
}

// ---- the session: info, the scoped_declare gate, states ----

#[tokio::test(start_paused = true)]
async fn says_info_first_and_declares_only_after_the_hub_announced_scoped_declare() {
    let mut rig = Rig::start(Stored::Config(config("test_pc", false)), None);
    assert_eq!(
        rig.next().await,
        Seen::Connected {
            session: 1,
            url: LOCAL.into(),
            node: "test_pc".into()
        }
    );
    assert_eq!(
        rig.frame("info").await,
        json!({
            "t": "info",
            "role": "service",
            "platform": "windows",
            "app": env!("CARGO_PKG_VERSION"),
        })
    );
    // Until Home Assistant answers there is no declare and no state.
    rig.nothing_more().await;
    assert_eq!(rig.state(), State::Connecting);

    rig.hub_sends(1, hub_info(&ALL_CAPS));
    let declare = rig.frame("declare").await;
    assert_eq!(declare["scope"], "machine");
    assert_eq!(declare["device"]["name"], "Test PC");
    let keys: Vec<&str> = declare["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entity| entity["key"].as_str().unwrap())
        .collect();
    for key in ["cpu", "uptime", "sleep", "shutdown", "restart", "hibernate"] {
        assert!(keys.contains(&key), "{key}");
    }
    assert!(!keys.contains(&"lock"), "nothing of a user's session");
    assert_eq!(
        rig.frame("state").await,
        json!({"t": "state", "s": {"cpu": "12.5"}})
    );

    let status = rig.state_becomes(State::Connected).await;
    assert_eq!(status.node_id, "test_pc");
    assert_eq!(status.fingerprint.len(), 64);
}

#[tokio::test(start_paused = true)]
async fn an_integration_without_scoped_declare_gets_no_declare() {
    let mut rig = Rig::start(Stored::Config(config("test_pc", false)), None);
    let session = rig.session_opens().await;
    rig.frame("info").await;
    rig.hub_sends(session, hub_info(&["info", "bye"]));

    // No declare, no bye: the connection just closes and the seat is free.
    rig.session_closes(session).await;
    assert_eq!(rig.next().await, Seen::SeatReleased);
    let status = rig.state_becomes(State::IntegrationTooOld).await;
    assert!(status.detail.contains("older than 0.9.0"));

    // The next look is an hour later, not earlier.
    let closed_at = Instant::now();
    rig.session_opens().await;
    assert_eq!(closed_at.elapsed(), TOO_OLD_RETRY);
    rig.frame("info").await;
}

#[tokio::test(start_paused = true)]
async fn an_integration_that_never_answers_info_gets_no_declare() {
    let mut rig = Rig::start(Stored::Config(config("test_pc", false)), None);
    let session = rig.session_opens().await;
    let asked_at = Instant::now();
    rig.frame("info").await;

    rig.session_closes(session).await;
    assert_eq!(asked_at.elapsed(), INFO_TIMEOUT);
    assert_eq!(rig.next().await, Seen::SeatReleased);
    rig.state_becomes(State::IntegrationTooOld).await;
}

#[tokio::test(start_paused = true)]
async fn an_info_answer_that_fails_the_checks_does_not_open_the_gate() {
    let mut rig = Rig::start(Stored::Config(config("test_pc", false)), None);
    let session = rig.session_opens().await;
    rig.frame("info").await;
    rig.hub_sends(
        session,
        json!({"t": "info", "integration": "latest", "caps": ["scoped_declare"]}),
    );
    rig.hub_sends(
        session,
        json!({"t": "info", "integration": "0.9.0", "caps": "scoped_declare"}),
    );
    rig.session_closes(session).await;
    rig.state_becomes(State::IntegrationTooOld).await;
}

#[tokio::test(start_paused = true)]
async fn sends_states_on_the_configured_interval() {
    let (mut rig, _) = Rig::connected().await;
    let first = Instant::now();
    assert_eq!(
        rig.frame("state").await,
        json!({"t": "state", "s": {"cpu": "12.5"}})
    );
    assert_eq!(first.elapsed(), Duration::from_secs(15));

    rig.readings
        .lock()
        .unwrap()
        .values
        .insert("cpu".into(), "40.0".into());
    let state = rig.frame_after_routine("state").await;
    assert_eq!(state["s"]["cpu"], "40.0");
    assert_eq!(first.elapsed(), Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn declares_again_when_the_detected_hardware_changes() {
    let (mut rig, _) = Rig::connected().await;
    rig.frame("state").await;

    {
        let mut readings = rig.readings.lock().unwrap();
        readings.hardware.push(gpu_sensor());
        readings.values.insert("gpu_usage".into(), "7".into());
    }
    // The second tick falls together with a ping; either may come first.
    let declare = loop {
        match rig.next().await {
            Seen::Frame { frame, .. } if frame["t"] == "ping" => {}
            Seen::Frame { frame, .. } => break frame,
            other => panic!("expected a frame, got {other:?}"),
        }
    };
    assert_eq!(declare["t"], "declare");
    assert_eq!(declare["scope"], "machine");
    assert!(declare["entities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entity| entity["key"] == "gpu_usage"));
    let state = rig.frame_after_routine("state").await;
    assert_eq!(state["s"]["gpu_usage"], "7");

    // Unchanged hardware is not declared again.
    assert_eq!(rig.frame_after_routine("state").await["t"], "state");
}

#[tokio::test(start_paused = true)]
async fn what_the_computer_offers_is_asked_off_the_runtime_once_per_session() {
    let mut rig = Rig::start_with(
        Stored::Config(config("test_pc", false)),
        Some(hub_info(&ALL_CAPS)),
        |rig| {
            rig.readings.lock().unwrap().offered = machine::Offered {
                hibernate: false,
                ..machine::Offered::ALL
            };
        },
    );
    rig.session_opens().await;
    rig.frame("info").await;
    let keys = |declare: &Value| -> Vec<String> {
        declare["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entity| entity["key"].as_str().unwrap().to_string())
            .collect()
    };
    let first = keys(&rig.frame("declare").await);
    assert!(first.contains(&"sleep".to_string()));
    assert!(!first.contains(&"hibernate".to_string()));
    rig.frame("state").await;

    rig.readings.lock().unwrap().hardware.push(gpu_sensor());
    let again = loop {
        match rig.next().await {
            Seen::Frame { frame, .. } if frame["t"] == "declare" => break frame,
            Seen::Frame { .. } => {}
            other => panic!("expected a frame, got {other:?}"),
        }
    };
    assert!(keys(&again).contains(&"gpu_usage".to_string()));
    assert!(!keys(&again).contains(&"hibernate".to_string()));

    let asked = rig.readings.lock().unwrap().offered_on.clone();
    assert_eq!(asked.len(), 1, "once per session, not per declare");
    // On Linux the answer comes from busctl, which may take seconds.
    assert_ne!(
        asked[0],
        std::thread::current().id(),
        "asked on a blocking thread, not on the runtime's"
    );
}

#[tokio::test(start_paused = true)]
async fn answers_a_ping_and_sends_its_own() {
    let (mut rig, session) = Rig::connected().await;
    rig.hub_sends(session, json!({"t": "ping"}));
    assert_eq!(rig.frame("pong").await, json!({"t": "pong"}));

    let start = Instant::now();
    loop {
        match rig.next().await {
            Seen::Frame { frame, .. } if frame["t"] == "state" => {}
            Seen::Frame { frame, .. } => {
                assert_eq!(frame, json!({"t": "ping"}));
                break;
            }
            other => panic!("expected a frame, got {other:?}"),
        }
    }
    assert_eq!(start.elapsed(), PING_INTERVAL);
    // A pong from Home Assistant needs no answer: the next thing the service
    // says beyond its routine is the pong to this second ping.
    rig.hub_sends(session, json!({"t": "pong"}));
    rig.hub_sends(session, json!({"t": "ping"}));
    assert_eq!(rig.frame_after_routine("pong").await, json!({"t": "pong"}));
}

// ---- commands and the safety valve ----

#[tokio::test(start_paused = true)]
async fn runs_a_power_command_and_records_it() {
    let (mut rig, session) = Rig::connected().await;
    let ack = rig.command(session, 7, "restart", "press").await;
    assert_eq!(ack, json!({"t": "ack", "id": 7, "ok": true}));
    assert_eq!(rig.commands_run(), ["restart"]);
    assert_eq!(
        rig.audit_lines(),
        ["power_command key=restart executor=fake result=ok"]
    );

    for (id, key) in [(8, "sleep"), (9, "hibernate"), (10, "shutdown")] {
        assert_eq!(rig.command(session, id, key, "press").await["ok"], true);
    }
    assert_eq!(
        rig.commands_run(),
        ["restart", "sleep", "hibernate", "shutdown"]
    );
}

#[tokio::test(start_paused = true)]
async fn refuses_power_commands_for_five_minutes_after_boot() {
    let (mut rig, session) = Rig::connected().await;
    rig.clock.set(60);
    let ack = rig.command(session, 1, "shutdown", "press").await;
    assert_eq!(ack["ok"], false);
    assert!(ack["error"].as_str().unwrap().contains("five minutes"));
    rig.clock.set(299);
    assert_eq!(
        rig.command(session, 2, "restart", "press").await["ok"],
        false
    );
    assert!(rig.commands_run().is_empty(), "nothing ran");
    assert_eq!(
        rig.audit_lines(),
        [
            "power_command key=shutdown result=refused_after_start",
            "power_command key=restart result=refused_after_start",
        ]
    );

    rig.clock.set(300);
    assert_eq!(
        rig.command(session, 3, "restart", "press").await["ok"],
        true
    );
    assert_eq!(rig.commands_run(), ["restart"]);
}

#[tokio::test(start_paused = true)]
async fn runs_nothing_but_the_four_power_commands() {
    let (mut rig, session) = Rig::connected().await;
    for (id, key) in [
        (1, "lock"),
        (2, "sign_out"),
        (3, "custom_backup"),
        (4, "type_text"),
        (5, ""),
        (6, "RESTART"),
    ] {
        let ack = rig.command(session, id, key, "press").await;
        assert_eq!(ack["ok"], false, "{key}");
        assert_eq!(ack["error"], UNSUPPORTED);
    }
    // A button is pressed; a power key with another action is refused too.
    let ack = rig.command(session, 7, "restart", "set").await;
    assert_eq!(ack["error"], "invalid command envelope");
    rig.hub_sends(session, json!({"t": "cmd", "id": 8, "key": "restart"}));
    assert_eq!(rig.frame_after_routine("ack").await["ok"], false);

    assert!(rig.commands_run().is_empty());
    // A key that is not one of the four never reaches the audit log.
    let audit = rig.audit_lines();
    assert_eq!(audit.len(), 8);
    assert!(audit[..6]
        .iter()
        .all(|line| line == "power_command result=refused_not_a_power_command"));
    assert_eq!(audit[6], "power_command key=restart result=refused_action");
}

#[tokio::test(start_paused = true)]
async fn refuses_a_command_before_the_hub_has_said_what_it_is() {
    let mut rig = Rig::start(Stored::Config(config("test_pc", false)), None);
    let session = rig.session_opens().await;
    rig.frame("info").await;
    rig.hub_sends(
        session,
        json!({"t": "cmd", "id": 1, "key": "shutdown", "action": "press"}),
    );
    let ack = rig.frame("ack").await;
    assert_eq!(ack["ok"], false);
    assert!(rig.commands_run().is_empty());
}

#[tokio::test(start_paused = true)]
async fn reports_a_command_that_could_not_be_started() {
    let (mut rig, session) = Rig::connected().await;
    rig.power.fails.store(true, Ordering::SeqCst);
    let ack = rig.command(session, 1, "sleep", "press").await;
    assert_eq!(
        ack,
        json!({"t": "ack", "id": 1, "ok": false, "error": "the command could not be started"})
    );
    assert_eq!(
        rig.audit_lines(),
        ["power_command key=sleep executor=fake result=error"]
    );
}

#[tokio::test(start_paused = true)]
async fn answers_requests_for_a_users_session_with_a_refusal() {
    let (mut rig, session) = Rig::connected().await;
    rig.hub_sends(
        session,
        json!({"t": "notify", "id": 3, "title": "Hello", "message": "there"}),
    );
    assert_eq!(
        rig.frame("ack").await,
        json!({"t": "ack", "id": 3, "ok": false, "error": UNSUPPORTED})
    );
    rig.hub_sends(session, json!({"t": "fs", "id": 4, "op": "roots"}));
    assert_eq!(
        rig.frame("fs_res").await,
        json!({"t": "fs_res", "id": 4, "ok": false, "error": UNSUPPORTED})
    );
    rig.hub_sends(session, json!({"t": "tv", "id": 5, "op": "apps"}));
    assert_eq!(rig.frame("tv_res").await["ok"], false);

    // A frame type the service does not know gets no answer.
    rig.hub_sends(session, json!({"t": "later", "id": 6}));
    rig.hub_sends(session, json!({"id": 7}));
    rig.nothing_more().await;
    assert_eq!(rig.state(), State::Connected);
}

// ---- the seat: handing over and taking back ----

#[tokio::test(start_paused = true)]
async fn hands_over_to_a_waiting_app_and_takes_the_seat_back() {
    let (mut rig, session) = Rig::connected().await;
    rig.seat.app_wants_it.store(true, Ordering::SeqCst);

    // The goodbye, then the close, and only then the seat.
    let asked = Instant::now();
    assert_eq!(
        rig.frame("bye").await,
        json!({"t": "bye", "reason": "handover"})
    );
    assert!(asked.elapsed() <= deskmate_core::seat::CHECK_EVERY);
    rig.session_closes(session).await;
    assert_eq!(rig.next().await, Seen::SeatReleased);
    let status = rig.state_becomes(State::Waiting).await;
    assert_eq!(status.detail, WAITING_FOR_APP);

    // While the app has the connection the service stays away.
    rig.nothing_for(Duration::from_secs(600)).await;

    rig.seat.app_wants_it.store(false, Ordering::SeqCst);
    let freed = Instant::now();
    assert_eq!(rig.session_opens().await, session + 1);
    assert!(freed.elapsed() <= SEAT_POLL);
    rig.frame("info").await;
    rig.frame("declare").await;
    rig.state_becomes(State::Connected).await;
}

#[tokio::test(start_paused = true)]
async fn a_session_change_makes_it_look_at_the_seat_at_once() {
    let (mut rig, session) = Rig::connected().await;
    rig.seat.app_wants_it.store(true, Ordering::SeqCst);
    let asked = Instant::now();
    rig.event(Event::SessionChange);
    assert_eq!(rig.frame("bye").await["reason"], "handover");
    assert_eq!(asked.elapsed(), Duration::ZERO);
    rig.session_closes(session).await;

    // And while it waits for the seat, a session change is a reason to look.
    assert_eq!(rig.next().await, Seen::SeatReleased);
    rig.nothing_for(Duration::from_secs(20)).await;
    rig.seat.app_wants_it.store(false, Ordering::SeqCst);
    let freed = Instant::now();
    rig.event(Event::SessionChange);
    rig.session_opens().await;
    assert_eq!(freed.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn stays_away_while_an_app_holds_the_seat_from_the_start() {
    let mut rig = Rig::start_with(
        Stored::Config(config("test_pc", false)),
        Some(hub_info(&ALL_CAPS)),
        |rig| rig.seat.app_wants_it.store(true, Ordering::SeqCst),
    );
    let status = rig.state_becomes(State::Waiting).await;
    assert_eq!(status.node_id, "test_pc");
    rig.nothing_for(Duration::from_secs(120)).await;

    rig.seat.app_wants_it.store(false, Ordering::SeqCst);
    rig.session_opens().await;
}

#[tokio::test(start_paused = true)]
async fn says_no_bye_to_an_integration_that_does_not_know_it() {
    let (mut rig, session) = Rig::connected_to(hub_info(&["info", "scoped_declare"])).await;
    rig.seat.app_wants_it.store(true, Ordering::SeqCst);
    rig.event(Event::SessionChange);
    rig.session_closes(session).await;
    assert_eq!(rig.next().await, Seen::SeatReleased);
}

#[tokio::test(start_paused = true)]
async fn gives_the_seat_to_an_app_that_starts_waiting_after_a_failed_attempt() {
    let mut rig = Rig::start_with(
        Stored::Config(config("test_pc", false)),
        Some(hub_info(&ALL_CAPS)),
        |rig| rig.hub.refusals.lock().unwrap().push_back(transport()),
    );
    assert_eq!(rig.next().await, Seen::Refused { url: LOCAL.into() });
    // The attempt took the seat, and nothing is open during the retry pause.
    assert!(rig.seat.held.load(Ordering::SeqCst));
    rig.seat.app_wants_it.store(true, Ordering::SeqCst);

    let asked = Instant::now();
    assert_eq!(rig.next().await, Seen::SeatReleased);
    assert!(asked.elapsed() <= Duration::from_secs(5));
    assert!(rig.seat.app_has_it());
    let status = rig.state_becomes(State::Waiting).await;
    assert_eq!(status.detail, WAITING_FOR_APP);
    rig.nothing_for(Duration::from_secs(600)).await;

    rig.seat.app_wants_it.store(false, Ordering::SeqCst);
    rig.session_opens().await;
}

#[tokio::test(start_paused = true)]
async fn gives_the_seat_to_an_app_that_starts_waiting_while_the_computer_sleeps() {
    let (mut rig, session) = Rig::connected().await;
    rig.event(Event::Suspend(Ack::none()));
    rig.frame("bye").await;
    rig.session_closes(session).await;
    rig.seat.app_wants_it.store(true, Ordering::SeqCst);

    rig.event(Event::Resume);
    assert_eq!(rig.next().await, Seen::SeatReleased);
    assert!(rig.seat.app_has_it());
    rig.nothing_for(Duration::from_secs(600)).await;
}

// ---- stop, shutdown, suspend, resume ----

#[tokio::test(start_paused = true)]
async fn a_stop_closes_the_session_without_a_bye() {
    let (mut rig, session) = Rig::connected().await;
    rig.event(Event::Stop);
    rig.session_closes(session).await;
    assert_eq!(rig.next().await, Seen::SeatReleased);
    rig.runner_ends().await;
}

#[tokio::test(start_paused = true)]
async fn a_shutdown_says_bye_and_ends_the_runner() {
    let (mut rig, session) = Rig::connected().await;
    rig.event(Event::Shutdown);
    assert_eq!(
        rig.frame("bye").await,
        json!({"t": "bye", "reason": "shutdown"})
    );
    rig.session_closes(session).await;
    rig.runner_ends().await;
}

#[tokio::test(start_paused = true)]
async fn a_suspend_says_bye_and_a_resume_reconnects_at_once() {
    let (mut rig, session) = Rig::connected().await;
    let (ack, waiter) = Ack::pair();
    rig.event(Event::Suspend(ack));
    assert_eq!(
        rig.frame("bye").await,
        json!({"t": "bye", "reason": "suspend"})
    );
    rig.session_closes(session).await;
    rig.state_becomes(State::Waiting).await;
    assert!(
        waiter.wait(Duration::ZERO),
        "whoever announced the suspend is told once the connection is closed"
    );

    // Asleep: no attempt until the system says it woke up.
    rig.nothing_for(Duration::from_secs(30)).await;
    let woke = Instant::now();
    rig.event(Event::Resume);
    assert_eq!(rig.session_opens().await, session + 1);
    assert_eq!(woke.elapsed(), Duration::ZERO);
    rig.frame("info").await;
    rig.frame("declare").await;
    rig.frame("state").await;

    // Waking up closes the valve again, however long the computer has been up.
    let ack = rig.command(session + 1, 1, "shutdown", "press").await;
    assert_eq!(ack["ok"], false);
    rig.clock.set(86_400 + 300);
    assert_eq!(
        rig.command(session + 1, 2, "shutdown", "press").await["ok"],
        true
    );
}

#[tokio::test(start_paused = true)]
async fn a_suspend_without_a_resume_does_not_keep_the_service_offline() {
    let (mut rig, session) = Rig::connected().await;
    rig.event(Event::Suspend(Ack::none()));
    rig.frame("bye").await;
    rig.session_closes(session).await;
    let closed = Instant::now();
    rig.session_opens().await;
    assert_eq!(closed.elapsed(), SUSPEND_FALLBACK);
}

#[tokio::test(start_paused = true)]
async fn a_resume_in_an_open_session_closes_the_valve_and_keeps_the_session() {
    let (mut rig, session) = Rig::connected().await;
    rig.event(Event::Resume);
    assert_eq!(rig.frame("ping").await, json!({"t": "ping"}));
    assert_eq!(
        rig.command(session, 1, "restart", "press").await["ok"],
        false
    );
    assert_eq!(rig.state(), State::Connected);
}

// ---- failures and the retry schedule ----

#[tokio::test(start_paused = true)]
async fn retries_a_network_failure_every_five_seconds() {
    let mut rig = Rig::start_with(
        Stored::Config(config("test_pc", false)),
        Some(hub_info(&ALL_CAPS)),
        |rig| {
            rig.hub
                .refusals
                .lock()
                .unwrap()
                .extend([transport(), transport(), transport()]);
        },
    );
    let start = Instant::now();
    for attempt in 0..3 {
        assert_eq!(rig.next().await, Seen::Refused { url: LOCAL.into() });
        assert_eq!(start.elapsed(), Duration::from_secs(5 * attempt));
    }
    let status = rig.state_becomes(State::Connecting).await;
    assert_eq!(
        status.detail,
        "Home Assistant cannot be reached. Trying again."
    );
    rig.session_opens().await;
    assert_eq!(start.elapsed(), Duration::from_secs(15));
}

#[tokio::test(start_paused = true)]
async fn falls_over_to_the_remote_address_after_two_failures() {
    let mut rig = Rig::start_with(
        Stored::Config(config("test_pc", true)),
        Some(hub_info(&ALL_CAPS)),
        |rig| {
            rig.hub
                .refusals
                .lock()
                .unwrap()
                .extend([transport(), transport(), transport()]);
        },
    );
    let start = Instant::now();
    assert_eq!(rig.next().await, Seen::Refused { url: LOCAL.into() });
    assert_eq!(rig.next().await, Seen::Refused { url: LOCAL.into() });
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert_eq!(rig.next().await, Seen::Refused { url: REMOTE.into() });
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    assert_eq!(
        rig.next().await,
        Seen::Connected {
            session: 1,
            url: REMOTE.into(),
            node: "test_pc".into()
        }
    );
    assert_eq!(start.elapsed(), Duration::from_secs(7));
}

#[tokio::test(start_paused = true)]
async fn a_rejected_pairing_is_an_error_and_is_retried_slowly() {
    let mut rig = Rig::start_with(
        Stored::Config(config("test_pc", false)),
        Some(hub_info(&ALL_CAPS)),
        |rig| {
            rig.hub.refusals.lock().unwrap().extend([
                refusal(LinkFailure::Auth),
                refusal(LinkFailure::Auth),
                refusal(LinkFailure::Auth),
                refusal(LinkFailure::Auth),
            ]);
        },
    );
    let start = Instant::now();
    let mut attempts = Vec::new();
    for _ in 0..4 {
        assert_eq!(rig.next().await, Seen::Refused { url: LOCAL.into() });
        attempts.push(start.elapsed().as_secs());
    }
    assert_eq!(attempts, [0, 5, 10, 25]);
    let status = rig.state_becomes(State::Error).await;
    assert!(status.detail.contains("rejected the pairing"));
    rig.session_opens().await;
    assert_eq!(start.elapsed(), Duration::from_secs(55));
    rig.state_becomes(State::Connected).await;
}

#[tokio::test(start_paused = true)]
async fn reconnects_after_home_assistant_closed_the_session() {
    let (mut rig, session) = Rig::connected().await;
    rig.hub_closes(session);
    rig.session_closes(session).await;
    let closed = Instant::now();
    rig.state_becomes(State::Connecting).await;
    assert_eq!(rig.session_opens().await, session + 1);
    assert_eq!(closed.elapsed(), Duration::from_secs(5));
    rig.frame("info").await;
    rig.frame("declare").await;
}

// ---- settings: provisioning, reload, deprovisioning ----

#[tokio::test(start_paused = true)]
async fn waits_unprovisioned_until_it_is_set_up() {
    let mut rig = Rig::start(Stored::Nothing, Some(hub_info(&ALL_CAPS)));
    rig.nothing_for(Duration::from_secs(3600)).await;
    let status = rig.status.borrow().clone();
    assert_eq!(status.state, State::NotProvisioned);
    assert_eq!(status.node_id, "");
    assert_eq!(status.fingerprint, "");

    rig.store(Stored::Config(config("test_pc", false)));
    rig.event(Event::Reload);
    rig.session_opens().await;
    rig.state_becomes(State::Connected).await;
}

#[tokio::test(start_paused = true)]
async fn shows_a_setup_it_cannot_use_and_looks_again_by_itself() {
    let mut rig = Rig::start(
        Stored::Broken("The pairing key is missing."),
        Some(hub_info(&ALL_CAPS)),
    );
    let status = rig.state_becomes(State::Error).await;
    assert_eq!(status.detail, "The pairing key is missing.");
    rig.nothing_for(Duration::from_secs(30)).await;

    rig.store(Stored::Config(config("test_pc", false)));
    let fixed = Instant::now();
    rig.session_opens().await;
    assert!(fixed.elapsed() <= INVALID_RECHECK);
}

#[tokio::test(start_paused = true)]
async fn a_reload_without_a_change_keeps_the_session() {
    let (mut rig, _) = Rig::connected().await;
    rig.event(Event::Reload);
    rig.nothing_more().await;
    assert_eq!(rig.state(), State::Connected);
}

#[tokio::test(start_paused = true)]
async fn a_reload_with_new_settings_reconnects_with_them() {
    let (mut rig, session) = Rig::connected().await;
    let before = rig.status.borrow().fingerprint.clone();
    rig.store(Stored::Config(config("other_pc", false)));
    rig.event(Event::Reload);

    // No bye: the service is not going away, it comes straight back.
    rig.session_closes(session).await;
    assert_eq!(rig.next().await, Seen::SeatReleased);
    assert_eq!(
        rig.next().await,
        Seen::Connected {
            session: session + 1,
            url: LOCAL.into(),
            node: "other_pc".into()
        }
    );
    let status = rig.state_becomes(State::Connected).await;
    assert_eq!(status.node_id, "other_pc");
    assert_ne!(status.fingerprint, before);
}

#[tokio::test(start_paused = true)]
async fn deprovisioning_disconnects_and_forgets_the_computer() {
    let (mut rig, session) = Rig::connected().await;
    rig.store(Stored::Nothing);
    rig.event(Event::Reload);
    rig.session_closes(session).await;
    assert_eq!(rig.next().await, Seen::SeatReleased);
    let status = rig.state_becomes(State::NotProvisioned).await;
    assert_eq!(status.node_id, "");
    assert_eq!(status.fingerprint, "");
    rig.nothing_for(Duration::from_secs(3600)).await;

    rig.event(Event::Stop);
    rig.runner_ends().await;
}

#[tokio::test(start_paused = true)]
async fn the_runner_ends_when_nobody_can_send_it_events_any_more() {
    let (mut rig, session) = Rig::connected().await;
    let (events, _) = mpsc::unbounded_channel();
    drop(std::mem::replace(&mut rig.events, events));
    rig.session_closes(session).await;
    rig.runner_ends().await;
}

#[test]
fn an_ack_tells_its_waiter_when_it_is_dropped() {
    let (ack, waiter) = Ack::pair();
    drop(ack);
    assert!(waiter.wait(Duration::ZERO));

    let (ack, waiter) = Ack::pair();
    assert!(!waiter.wait(Duration::from_millis(10)));
    drop(ack);
}

// ---- the Linux daemon ----

#[tokio::test(start_paused = true)]
async fn the_daemon_says_what_it_is_and_names_itself_in_its_refusals() {
    let mut rig = Rig::start_as(
        ClientRole::Daemon,
        ClientPlatform::Linux,
        Stored::Config(config("test_pc", false)),
        Some(hub_info(&ALL_CAPS)),
        |_| {},
    );
    let session = rig.session_opens().await;
    assert_eq!(
        rig.frame("info").await,
        json!({
            "t": "info",
            "role": "daemon",
            "platform": "linux",
            "app": env!("CARGO_PKG_VERSION"),
        })
    );
    assert_eq!(rig.frame("declare").await["scope"], "machine");
    rig.frame("state").await;

    let refusal = "not supported by the Deskmate daemon";
    rig.hub_sends(
        session,
        json!({"t": "notify", "id": 3, "title": "Hello", "message": "there"}),
    );
    assert_eq!(
        rig.frame("ack").await,
        json!({"t": "ack", "id": 3, "ok": false, "error": refusal})
    );
    rig.hub_sends(session, json!({"t": "fs", "id": 4, "op": "roots"}));
    assert_eq!(
        rig.frame("fs_res").await,
        json!({"t": "fs_res", "id": 4, "ok": false, "error": refusal})
    );
    let ack = rig.command(session, 5, "lock", "press").await;
    assert_eq!(ack["error"], refusal);
    let ack = rig.command(session, 6, "restart", "press").await;
    assert_eq!(ack, json!({"t": "ack", "id": 6, "ok": true}));
    assert_eq!(rig.commands_run(), ["restart"]);
}

#[tokio::test(start_paused = true)]
async fn the_daemon_tells_how_to_pair_it() {
    let mut rig = Rig::start_as(
        ClientRole::Daemon,
        ClientPlatform::Linux,
        Stored::Nothing,
        None,
        |_| {},
    );
    let status = rig.state_becomes(State::NotProvisioned).await;
    assert!(
        status.detail.contains("sudo deskmate-service pair"),
        "{}",
        status.detail
    );

    rig.hub
        .refusals
        .lock()
        .unwrap()
        .push_back(refusal(LinkFailure::Auth));
    rig.store(Stored::Config(config("test_pc", false)));
    rig.event(Event::Reload);
    let status = rig.state_becomes(State::Error).await;
    assert!(
        status.detail.contains("Pair the daemon again"),
        "{}",
        status.detail
    );
}
