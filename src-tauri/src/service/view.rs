//! What the Settings panel and the Status page show about the Deskmate
//! service, worked out from what Windows and the service's status line say.
//! Nothing here touches the system, so every case is tested.

use serde::Serialize;

/// The status line's format version this app reads.
const STATUS_VERSION: u64 = 1;
const MAX_VERSION_CHARS: usize = 40;
const MAX_NODE_CHARS: usize = 64;
const MAX_DETAIL_CHARS: usize = 200;
const FINGERPRINT_HEX_CHARS: usize = 64;

/// Where the service stands by its own account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportedState {
    NotProvisioned,
    Waiting,
    Connecting,
    Connected,
    IntegrationTooOld,
    Error,
}

impl ReportedState {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "not_provisioned" => Self::NotProvisioned,
            "waiting" => Self::Waiting,
            "connecting" => Self::Connecting,
            "connected" => Self::Connected,
            "integration_too_old" => Self::IntegrationTooOld,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

/// A status line that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub version: String,
    pub state: ReportedState,
    pub node_id: String,
    pub detail: String,
    /// 64 lowercase hex digits, or empty without settings.
    pub fingerprint: String,
}

/// Three numbers separated by dots, with an optional `-` or `+` suffix of
/// letters, digits, dots and hyphens.
fn is_version(text: &str) -> bool {
    text.len() <= MAX_VERSION_CHARS && version_parts(text).is_some()
}

fn version_parts(text: &str) -> Option<([u64; 3], bool)> {
    let (numbers, suffix) = match text.find(['-', '+']) {
        Some(at) => (&text[..at], Some((&text[at..at + 1], &text[at + 1..]))),
        None => (text, None),
    };
    let mut parts = numbers.split('.');
    let mut triple = [0u64; 3];
    for slot in &mut triple {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    let pre_release = match suffix {
        None => false,
        Some((_, rest))
            if rest.is_empty()
                || !rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') =>
        {
            return None
        }
        Some((sign, _)) => sign == "-",
    };
    Some((triple, pre_release))
}

/// Whether `offered` is a later version than `running`. A pre-release comes
/// before its release; two versions that differ only in what follows them
/// count as the same. Anything unreadable is never newer.
pub fn is_newer(running: &str, offered: &str) -> bool {
    match (version_parts(running), version_parts(offered)) {
        (Some((running, running_pre)), Some((offered, offered_pre))) => {
            offered > running || (offered == running && running_pre && !offered_pre)
        }
        _ => false,
    }
}

fn is_node(text: &str) -> bool {
    text.len() <= MAX_NODE_CHARS
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_fingerprint(text: &str) -> bool {
    text.is_empty()
        || (text.len() == FINGERPRINT_HEX_CHARS
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// Reads the service's status line. Every field is checked before any of it
/// is shown, because a program that took the pipe's name could send
/// anything. The error never repeats what was received.
pub fn parse_status_line(line: &str) -> Result<StatusLine, String> {
    const UNREADABLE: &str = "The service gave an answer this Deskmate does not understand.";
    let value: serde_json::Value = serde_json::from_str(line.trim()).map_err(|_| UNREADABLE)?;
    let text = |field: &str| value.get(field).and_then(serde_json::Value::as_str);
    if value.get("v").and_then(serde_json::Value::as_u64) != Some(STATUS_VERSION) {
        return Err(UNREADABLE.into());
    }
    let version = text("version")
        .filter(|v| is_version(v))
        .ok_or(UNREADABLE)?;
    let state = text("state")
        .and_then(ReportedState::parse)
        .ok_or(UNREADABLE)?;
    let node_id = text("node_id").filter(|n| is_node(n)).ok_or(UNREADABLE)?;
    let detail = text("detail")
        .filter(|d| d.chars().count() <= MAX_DETAIL_CHARS && !d.chars().any(char::is_control))
        .ok_or(UNREADABLE)?;
    let fingerprint = text("config")
        .filter(|f| is_fingerprint(f))
        .ok_or(UNREADABLE)?;
    Ok(StatusLine {
        version: version.into(),
        state,
        node_id: node_id.into(),
        detail: detail.into(),
        fingerprint: fingerprint.into(),
    })
}

/// What the service manager says about the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    NotRegistered,
    Registered,
}

/// What the app knows about itself that decides whether the service can be
/// set up from here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conditions {
    /// This build has `service\deskmate-service.exe` next to the app.
    pub bundled: bool,
    /// The app connects through the Deskmate integration, not MQTT.
    pub link: bool,
    /// A pairing key is stored for this computer.
    pub paired: bool,
    /// Why the settings or the keys cannot be handed over, if they cannot.
    pub settings_problem: Option<String>,
    /// Whether the connected integration announced `scoped_declare`; None
    /// while no Link session has told.
    pub scoped_declare: Option<bool>,
    /// The user pressed Disconnect.
    pub paused: bool,
}

/// Whether the integration of the current Link session can take a service:
/// what its `info` answer says, or None while nothing tells. An integration
/// older than 0.9.0 never answers `info`, so a session that is connected but
/// has no answer counts as one that cannot. That also holds for the first few
/// seconds of a session before the answer arrives, which the panel refreshes
/// past.
pub fn integration_supports_service(connected: bool, answered: Option<bool>) -> Option<bool> {
    match answered {
        Some(supports) => Some(supports),
        None if connected => Some(false),
        None => None,
    }
}

/// Why the service cannot be installed from here, or None when it can.
pub fn install_blocked(conditions: &Conditions) -> Option<String> {
    if let Some(reason) = setup_blocked(conditions) {
        return Some(reason);
    }
    match conditions.scoped_declare {
        Some(true) => None,
        Some(false) => Some(
            "The Deskmate integration in Home Assistant does not support the service yet. Update it to 0.9.0 or newer in HACS, restart Home Assistant, then connect again."
                .into(),
        ),
        None => Some(
            "Connect to Home Assistant first, so Deskmate can check that the integration supports the service."
                .into(),
        ),
    }
}

/// Why the settings cannot be handed to the service, or None when they can.
/// The integration's version is not checked here: the service checks it
/// itself and says so.
fn setup_blocked(conditions: &Conditions) -> Option<String> {
    if !conditions.bundled {
        return Some(
            "This copy of Deskmate does not include the service. Install Deskmate from its installer."
                .into(),
        );
    }
    if !conditions.link {
        return Some("The service works with the Deskmate integration only, not with MQTT.".into());
    }
    if !conditions.paired {
        return Some("Pair this computer with Home Assistant first.".into());
    }
    conditions.settings_problem.clone()
}

/// The headline state of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotInstalled,
    /// Registered, but nothing answers on the status pipe.
    NotAnswering,
    Running,
    NeedsUpdate,
    SettingsDiffer,
    IntegrationTooOld,
    Error,
}

/// What the service is doing while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Connected,
    Waiting,
    Connecting,
}

/// Everything the window shows about the service. Mirrored in `src/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceView {
    pub phase: Phase,
    pub activity: Option<Activity>,
    /// The service's own short text, or the app's for a state it worked out.
    pub detail: String,
    pub running_version: Option<String>,
    pub bundled_version: String,
    /// The copy in this app is newer than the one that runs.
    pub needs_update: bool,
    /// The service holds other settings or keys than the app, or none.
    pub settings_differ: bool,
    /// The running service is newer than the copy in this app.
    pub newer_than_app: bool,
    /// Why Install is off, or null.
    pub install_blocked: Option<String>,
    /// Why Apply is off, or null.
    pub apply_blocked: Option<String>,
    /// Why Update is off, or null.
    pub update_blocked: Option<String>,
    /// The service waits because the user pressed Disconnect.
    pub paused_by_user: bool,
}

/// What was observed, in one place.
pub struct Observed<'a> {
    pub registration: Registration,
    /// None: nothing answered on the status pipe.
    pub line: Option<Result<StatusLine, String>>,
    /// What the service's fingerprint should be; None when the app cannot
    /// tell (not paired, MQTT, settings the service would refuse).
    pub expected_fingerprint: Option<&'a str>,
    pub bundled_version: &'a str,
    pub conditions: &'a Conditions,
}

const NEWER_THAN_APP: &str =
    "The installed service is newer than the copy in this Deskmate. Update Deskmate first.";

pub fn build_view(observed: &Observed<'_>) -> ServiceView {
    let conditions = observed.conditions;
    let mut view = ServiceView {
        phase: Phase::NotInstalled,
        activity: None,
        detail: String::new(),
        running_version: None,
        bundled_version: observed.bundled_version.to_string(),
        needs_update: false,
        settings_differ: false,
        newer_than_app: false,
        install_blocked: install_blocked(conditions),
        apply_blocked: setup_blocked(conditions),
        update_blocked: (!conditions.bundled)
            .then(|| "This copy of Deskmate does not include the service.".to_string()),
        paused_by_user: false,
    };
    if observed.registration == Registration::NotRegistered {
        view.detail = "The service is not installed.".into();
        return view;
    }
    let line = match &observed.line {
        None => {
            view.phase = Phase::NotAnswering;
            view.detail =
                "The service is installed but does not answer. Update starts it again.".into();
            return view;
        }
        Some(Err(problem)) => {
            view.phase = Phase::Error;
            view.detail = problem.clone();
            return view;
        }
        Some(Ok(line)) => line,
    };
    view.running_version = Some(line.version.clone());
    view.needs_update = is_newer(&line.version, observed.bundled_version);
    view.newer_than_app = is_newer(observed.bundled_version, &line.version);
    if view.newer_than_app {
        view.apply_blocked = Some(NEWER_THAN_APP.into());
        view.update_blocked = Some(NEWER_THAN_APP.into());
    }
    view.settings_differ = line.state == ReportedState::NotProvisioned
        || observed
            .expected_fingerprint
            .is_some_and(|expected| expected != line.fingerprint);
    view.activity = match line.state {
        ReportedState::Connected => Some(Activity::Connected),
        ReportedState::Waiting => Some(Activity::Waiting),
        ReportedState::Connecting => Some(Activity::Connecting),
        _ => None,
    };
    view.paused_by_user = line.state == ReportedState::Waiting && conditions.paused;
    view.detail = line.detail.clone();
    view.phase = match line.state {
        ReportedState::Error => Phase::Error,
        ReportedState::IntegrationTooOld => Phase::IntegrationTooOld,
        _ if view.needs_update => Phase::NeedsUpdate,
        _ if view.settings_differ => Phase::SettingsDiffer,
        _ => Phase::Running,
    };
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn line_json(state: &str, version: &str, config: &str) -> String {
        json!({
            "v": 1,
            "version": version,
            "state": state,
            "node_id": "office_pc",
            "detail": "Connected to Home Assistant.",
            "config": config,
        })
        .to_string()
    }

    fn ready() -> Conditions {
        Conditions {
            bundled: true,
            link: true,
            paired: true,
            settings_problem: None,
            scoped_declare: Some(true),
            paused: false,
        }
    }

    fn view(state: &str, version: &str, config: &str, conditions: &Conditions) -> ServiceView {
        build_view(&Observed {
            registration: Registration::Registered,
            line: Some(parse_status_line(&line_json(state, version, config))),
            expected_fingerprint: Some(FINGERPRINT),
            bundled_version: "0.9.0",
            conditions,
        })
    }

    #[test]
    fn reads_the_status_line_the_service_writes() {
        let line = parse_status_line(&line_json("connected", "0.9.0", FINGERPRINT)).unwrap();
        assert_eq!(line.state, ReportedState::Connected);
        assert_eq!(line.version, "0.9.0");
        assert_eq!(line.node_id, "office_pc");
        assert_eq!(line.fingerprint, FINGERPRINT);
        for state in [
            "not_provisioned",
            "waiting",
            "connecting",
            "connected",
            "integration_too_old",
            "error",
        ] {
            assert!(
                parse_status_line(&line_json(state, "0.9.0", "")).is_ok(),
                "{state}"
            );
        }
    }

    #[test]
    fn refuses_a_status_line_it_cannot_trust() {
        let good: serde_json::Value =
            serde_json::from_str(&line_json("waiting", "0.9.0", "")).unwrap();
        let with = |field: &str, value: serde_json::Value| {
            let mut changed = good.clone();
            changed[field] = value;
            changed.to_string()
        };
        let marker = "repeat-me";
        let lines = [
            "".to_string(),
            "not json".into(),
            "[1]".into(),
            with("v", json!(2)),
            with("version", json!("latest")),
            with("version", json!("1.2.3.4")),
            with("version", json!("9".repeat(41))),
            with("state", json!("running")),
            with("state", json!(marker)),
            with("node_id", json!("Office PC")),
            with("detail", json!("first\nsecond")),
            with("detail", json!("d".repeat(201))),
            with("config", json!("ABCDEF")),
            with("config", json!(&FINGERPRINT[1..])),
            with("config", json!(format!("{marker}{}", &FINGERPRINT[9..]))),
        ];
        for line in lines {
            let error = parse_status_line(&line).unwrap_err();
            assert!(!error.contains(marker), "{error}");
        }
    }

    #[test]
    fn compares_versions_for_an_update() {
        assert!(is_newer("0.9.0", "0.9.1"));
        assert!(is_newer("0.9.9", "0.10.0"));
        assert!(is_newer("0.9.0", "1.0.0"));
        assert!(is_newer("0.9.0-beta.1", "0.9.0"));
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("0.9.1", "0.9.0"));
        assert!(!is_newer("0.9.0", "0.9.0-beta.1"));
        assert!(!is_newer("0.9.0", "0.9.0+rebuilt"));
        assert!(!is_newer("0.9.0", "latest"));
        assert!(!is_newer("unknown", "9.9.9"));
        assert!(!is_newer("0.9.0", "1.0"));
        assert!(!is_newer("0.9.0", "1.0.0-"));
    }

    #[test]
    fn maps_every_reported_state_to_a_panel_state() {
        let conditions = ready();
        let running = view("connected", "0.9.0", FINGERPRINT, &conditions);
        assert_eq!(running.phase, Phase::Running);
        assert_eq!(running.activity, Some(Activity::Connected));
        assert!(!running.needs_update && !running.settings_differ);
        assert_eq!(running.running_version.as_deref(), Some("0.9.0"));

        assert_eq!(
            view("waiting", "0.9.0", FINGERPRINT, &conditions).activity,
            Some(Activity::Waiting)
        );
        assert_eq!(
            view("connecting", "0.9.0", FINGERPRINT, &conditions).activity,
            Some(Activity::Connecting)
        );
        let too_old = view("integration_too_old", "0.9.0", FINGERPRINT, &conditions);
        assert_eq!(too_old.phase, Phase::IntegrationTooOld);
        assert_eq!(too_old.activity, None);
        let error = view("error", "0.9.0", "", &conditions);
        assert_eq!(error.phase, Phase::Error);
        assert_eq!(error.detail, "Connected to Home Assistant.");

        let unprovisioned = view("not_provisioned", "0.9.0", "", &conditions);
        assert_eq!(unprovisioned.phase, Phase::SettingsDiffer);
        assert!(unprovisioned.settings_differ);
    }

    #[test]
    fn a_different_fingerprint_means_the_settings_differ() {
        let conditions = ready();
        let other = "f".repeat(64);
        let differ = view("connected", "0.9.0", &other, &conditions);
        assert_eq!(differ.phase, Phase::SettingsDiffer);
        assert!(differ.settings_differ);
        assert_eq!(differ.apply_blocked, None);

        // Without its own fingerprint the app does not claim a difference.
        let unknown = build_view(&Observed {
            registration: Registration::Registered,
            line: Some(parse_status_line(&line_json("connected", "0.9.0", &other))),
            expected_fingerprint: None,
            bundled_version: "0.9.0",
            conditions: &conditions,
        });
        assert!(!unknown.settings_differ);
        assert_eq!(unknown.phase, Phase::Running);
    }

    #[test]
    fn an_older_service_needs_an_update_and_a_newer_one_blocks_changes() {
        let conditions = ready();
        let old = view("connected", "0.8.9", FINGERPRINT, &conditions);
        assert_eq!(old.phase, Phase::NeedsUpdate);
        assert!(old.needs_update && !old.newer_than_app);
        assert_eq!(old.update_blocked, None);

        // An update outranks a settings difference; Apply updates as well.
        let both = view("connected", "0.8.9", &"f".repeat(64), &conditions);
        assert_eq!(both.phase, Phase::NeedsUpdate);
        assert!(both.settings_differ);

        let newer = view("connected", "0.9.1", &"f".repeat(64), &conditions);
        assert!(newer.newer_than_app && !newer.needs_update);
        assert!(newer.apply_blocked.is_some() && newer.update_blocked.is_some());
        // An error is still shown as an error.
        assert_eq!(view("error", "0.8.9", "", &conditions).phase, Phase::Error);
    }

    #[test]
    fn a_service_that_is_missing_or_silent() {
        let conditions = ready();
        let missing = build_view(&Observed {
            registration: Registration::NotRegistered,
            line: None,
            expected_fingerprint: None,
            bundled_version: "0.9.0",
            conditions: &conditions,
        });
        assert_eq!(missing.phase, Phase::NotInstalled);
        assert_eq!(missing.install_blocked, None);

        let silent = build_view(&Observed {
            registration: Registration::Registered,
            line: None,
            expected_fingerprint: Some(FINGERPRINT),
            bundled_version: "0.9.0",
            conditions: &conditions,
        });
        assert_eq!(silent.phase, Phase::NotAnswering);
        assert_eq!(silent.running_version, None);

        let garbled = build_view(&Observed {
            registration: Registration::Registered,
            line: Some(parse_status_line("garbage")),
            expected_fingerprint: Some(FINGERPRINT),
            bundled_version: "0.9.0",
            conditions: &conditions,
        });
        assert_eq!(garbled.phase, Phase::Error);
    }

    #[test]
    fn says_why_install_is_off() {
        assert_eq!(install_blocked(&ready()), None);
        let cases = [
            (
                Conditions {
                    bundled: false,
                    ..ready()
                },
                "does not include",
            ),
            (
                Conditions {
                    link: false,
                    ..ready()
                },
                "MQTT",
            ),
            (
                Conditions {
                    paired: false,
                    ..ready()
                },
                "Pair",
            ),
            (
                Conditions {
                    settings_problem: Some("The cascade key is missing.".into()),
                    ..ready()
                },
                "cascade key",
            ),
            (
                Conditions {
                    scoped_declare: Some(false),
                    ..ready()
                },
                "HACS",
            ),
            (
                Conditions {
                    scoped_declare: None,
                    ..ready()
                },
                "Connect",
            ),
        ];
        for (conditions, expected) in cases {
            let reason = install_blocked(&conditions).unwrap_or_default();
            assert!(!reason.is_empty(), "{expected}");
            assert!(reason.contains(expected), "{reason}");
        }
        // Apply does not wait for the integration's answer: the service
        // checks the integration itself.
        let unknown = Conditions {
            scoped_declare: None,
            ..ready()
        };
        assert_eq!(setup_blocked(&unknown), None);
        assert!(setup_blocked(&Conditions {
            link: false,
            ..ready()
        })
        .is_some());
    }

    #[test]
    fn says_when_the_service_waits_because_of_disconnect() {
        let paused = Conditions {
            paused: true,
            ..ready()
        };
        assert!(view("waiting", "0.9.0", FINGERPRINT, &paused).paused_by_user);
        assert!(!view("connected", "0.9.0", FINGERPRINT, &paused).paused_by_user);
        assert!(!view("waiting", "0.9.0", FINGERPRINT, &ready()).paused_by_user);
    }

    #[test]
    fn a_connected_integration_that_never_answers_info_cannot_take_the_service() {
        // An integration up to 0.8.5 is connected but never answers `info`.
        assert_eq!(integration_supports_service(true, None), Some(false));
        assert_eq!(integration_supports_service(false, None), None);
        assert_eq!(integration_supports_service(true, Some(true)), Some(true));
        assert_eq!(integration_supports_service(true, Some(false)), Some(false));
        let reason = install_blocked(&Conditions {
            scoped_declare: integration_supports_service(true, None),
            ..ready()
        })
        .unwrap_or_default();
        assert!(reason.contains("0.9.0"), "{reason}");
    }
}
