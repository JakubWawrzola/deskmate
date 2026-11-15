//! How the hotkeys and tray quick actions of the configuration appear on the
//! channel: `hotkey:<id>` and `tray:<id>`, with their names.

use serde_json::{json, Value};

use super::backend::ActionEntry;
use super::protocol::is_valid_action_id;
use crate::config::{ActionSpec, AppConfig};

const HOTKEY_PREFIX: &str = "hotkey:";
const TRAY_PREFIX: &str = "tray:";

/// Longest name shown for an action; a longer one is cut.
const MAX_NAME_CHARS: usize = 255;

/// What an action id names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionSource {
    Hotkey,
    Tray,
}

/// An action of the configuration, found by its channel id.
#[derive(Debug, Clone)]
pub struct FoundAction {
    pub source: ActionSource,
    /// The id of the hotkey or tray action itself, without the prefix.
    pub source_id: String,
    pub spec: ActionSpec,
}

/// Every hotkey and tray quick action, hotkeys first, each in the order of
/// the configuration. An entry whose id `run` would refuse is left out.
pub fn list(cfg: &AppConfig) -> Vec<ActionEntry> {
    let hotkeys = cfg
        .hotkeys
        .iter()
        .map(|hotkey| entry(HOTKEY_PREFIX, &hotkey.id, &hotkey.name));
    let tray = cfg
        .tray_actions
        .iter()
        .map(|action| entry(TRAY_PREFIX, &action.id, &action.name));
    hotkeys
        .chain(tray)
        .filter(|entry| is_listable(&entry.id))
        .collect()
}

fn entry(prefix: &str, id: &str, name: &str) -> ActionEntry {
    let name = name.trim();
    ActionEntry {
        id: format!("{prefix}{id}"),
        name: if name.is_empty() {
            id.to_string()
        } else {
            name.chars().take(MAX_NAME_CHARS).collect()
        },
    }
}

/// An id with a non-empty part after the prefix, of the shape `run` accepts.
fn is_listable(action_id: &str) -> bool {
    is_valid_action_id(action_id)
        && action_id
            .split_once(':')
            .is_some_and(|(_, id)| !id.is_empty() && !id.contains(':'))
}

/// The action an id names, if the configuration has it.
pub fn find(cfg: &AppConfig, action_id: &str) -> Option<FoundAction> {
    if !is_listable(action_id) {
        return None;
    }
    if let Some(id) = action_id.strip_prefix(HOTKEY_PREFIX) {
        return cfg
            .hotkeys
            .iter()
            .find(|hotkey| hotkey.id == id)
            .map(|hotkey| FoundAction {
                source: ActionSource::Hotkey,
                source_id: hotkey.id.clone(),
                spec: hotkey.action.clone(),
            });
    }
    if let Some(id) = action_id.strip_prefix(TRAY_PREFIX) {
        return cfg
            .tray_actions
            .iter()
            .find(|action| action.id == id)
            .map(|action| FoundAction {
                source: ActionSource::Tray,
                source_id: action.id.clone(),
                spec: action.action.clone(),
            });
    }
    None
}

/// One action as it appears in the answer to `actions`.
pub fn to_json(entry: &ActionEntry) -> Value {
    json!({ "id": entry.id, "name": entry.name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Hotkey, TrayAction};

    fn spec(kind: &str) -> ActionSpec {
        ActionSpec {
            kind: kind.into(),
            ..ActionSpec::default()
        }
    }

    fn config() -> AppConfig {
        AppConfig {
            hotkeys: vec![
                Hotkey {
                    id: "lights".into(),
                    name: "Lights".into(),
                    accelerator: "Ctrl+Alt+L".into(),
                    action: spec("toggle"),
                },
                Hotkey {
                    id: "pc_sleep".into(),
                    name: "  ".into(),
                    accelerator: String::new(),
                    action: spec("command"),
                },
            ],
            tray_actions: vec![TrayAction {
                id: "pc_sleep".into(),
                name: "Sleep".into(),
                action: spec("widget"),
            }],
            ..AppConfig::default()
        }
    }

    #[test]
    fn hotkeys_and_tray_actions_are_listed_with_their_prefix_and_name() {
        assert_eq!(
            list(&config()),
            [
                ActionEntry {
                    id: "hotkey:lights".into(),
                    name: "Lights".into()
                },
                // Without a name of its own, the id stands in.
                ActionEntry {
                    id: "hotkey:pc_sleep".into(),
                    name: "pc_sleep".into()
                },
                ActionEntry {
                    id: "tray:pc_sleep".into(),
                    name: "Sleep".into()
                },
            ]
        );
    }

    #[test]
    fn the_prefix_says_which_list_an_id_is_looked_up_in() {
        let cfg = config();
        let hotkey = find(&cfg, "hotkey:pc_sleep").unwrap();
        assert_eq!(hotkey.source, ActionSource::Hotkey);
        assert_eq!(hotkey.source_id, "pc_sleep");
        assert_eq!(hotkey.spec.kind, "command");

        let tray = find(&cfg, "tray:pc_sleep").unwrap();
        assert_eq!(tray.source, ActionSource::Tray);
        assert_eq!(tray.source_id, "pc_sleep");
        assert_eq!(tray.spec.kind, "widget");
    }

    #[test]
    fn an_id_that_names_nothing_is_not_found() {
        let cfg = config();
        for action_id in [
            "hotkey:gone",
            "tray:lights",
            "lights",
            "hotkey:",
            "tray:",
            "command:lock",
            "hotkey:lights:extra",
            "HOTKEY:lights",
            "hotkey:Lights",
            "",
        ] {
            assert!(find(&cfg, action_id).is_none(), "{action_id:?}");
        }
    }

    #[test]
    fn every_listed_action_can_be_found() {
        let cfg = config();
        for entry in list(&cfg) {
            assert!(find(&cfg, &entry.id).is_some(), "{}", entry.id);
        }
    }

    #[test]
    fn an_entry_with_an_id_that_run_would_refuse_is_not_listed() {
        let mut cfg = config();
        cfg.hotkeys[0].id = "Lights On".into();
        cfg.hotkeys[1].id = String::new();
        cfg.tray_actions[0].id = "a".repeat(200);
        assert!(list(&cfg).is_empty());
    }

    #[test]
    fn an_overlong_name_is_cut() {
        let mut cfg = config();
        cfg.hotkeys[0].name = "n".repeat(1_000);
        assert_eq!(list(&cfg)[0].name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn an_action_is_written_as_in_the_reference() {
        assert_eq!(
            to_json(&ActionEntry {
                id: "hotkey:lights".into(),
                name: "Lights".into()
            }),
            json!({"id": "hotkey:lights", "name": "Lights"})
        );
    }
}
