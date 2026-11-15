//! What the service says about itself: one JSON line for whoever asks.
//!
//! Any local user may read it, so it carries the state, the computer's id and
//! a short fixed text, and never an address, a key or anything Home Assistant
//! sent. The fingerprint lets the app compare settings without seeing them
//! (`ServiceConfig::fingerprint`).

use serde_json::json;

/// Version of the status line's format.
pub const STATUS_VERSION: u64 = 1;

/// Where the service stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No settings yet.
    NotProvisioned,
    /// Provisioned, but not trying to connect right now: the app holds the
    /// connection, or the computer is going to sleep.
    Waiting,
    /// Connecting, or about to try again after a network failure.
    Connecting,
    /// A session with Home Assistant is open.
    Connected,
    /// The integration in Home Assistant is older than 0.9.0.
    IntegrationTooOld,
    /// Something a person has to fix: the settings, a key, the pairing.
    Error,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotProvisioned => "not_provisioned",
            Self::Waiting => "waiting",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::IntegrationTooOld => "integration_too_old",
            Self::Error => "error",
        }
    }
}

/// One snapshot of the service's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    /// Empty until the service is provisioned.
    pub node_id: String,
    /// A short English text for a person.
    pub detail: String,
    /// Fingerprint of the settings and keys in use; empty without them.
    pub fingerprint: String,
}

impl Status {
    pub fn not_provisioned() -> Self {
        Self {
            state: State::NotProvisioned,
            node_id: String::new(),
            detail: "The service has no settings yet. Set it up from the Deskmate app.".into(),
            fingerprint: String::new(),
        }
    }

    /// The status as one line of JSON, with the line break.
    pub fn line(&self) -> String {
        let mut line = json!({
            "v": STATUS_VERSION,
            "version": env!("CARGO_PKG_VERSION"),
            "state": self.state.as_str(),
            "node_id": self.node_id,
            "detail": self.detail,
            "config": self.fingerprint,
        })
        .to_string();
        line.push('\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn the_line_is_one_json_object_with_exactly_the_documented_fields() {
        let status = Status {
            state: State::Connected,
            node_id: "office_pc".into(),
            detail: "Connected to Home Assistant.".into(),
            fingerprint: "ab".repeat(32),
        };
        let line = status.line();
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            value,
            json!({
                "v": 1,
                "version": env!("CARGO_PKG_VERSION"),
                "state": "connected",
                "node_id": "office_pc",
                "detail": "Connected to Home Assistant.",
                "config": "ab".repeat(32),
            })
        );
    }

    #[test]
    fn every_state_has_its_documented_name() {
        let names: Vec<&str> = [
            State::NotProvisioned,
            State::Waiting,
            State::Connecting,
            State::Connected,
            State::IntegrationTooOld,
            State::Error,
        ]
        .into_iter()
        .map(State::as_str)
        .collect();
        assert_eq!(
            names,
            [
                "not_provisioned",
                "waiting",
                "connecting",
                "connected",
                "integration_too_old",
                "error"
            ]
        );
    }

    #[test]
    fn an_unprovisioned_service_shows_no_node_and_no_fingerprint() {
        let value: Value = serde_json::from_str(&Status::not_provisioned().line()).unwrap();
        assert_eq!(value["state"], "not_provisioned");
        assert_eq!(value["node_id"], "");
        assert_eq!(value["config"], "");
    }

    #[test]
    fn a_line_break_in_a_text_cannot_split_the_line() {
        let status = Status {
            state: State::Error,
            node_id: "pc".into(),
            detail: "first\nsecond".into(),
            fingerprint: String::new(),
        };
        assert_eq!(status.line().matches('\n').count(), 1);
    }
}
