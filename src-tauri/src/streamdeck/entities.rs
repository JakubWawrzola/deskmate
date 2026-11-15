//! The entity list of the `entities` request: what Home Assistant reported,
//! checked, filtered by domain and put in a stable order.

use serde_json::{json, Value};

use super::backend::EntityInfo;
use super::protocol::is_valid_entity_id;
use crate::ha_api::EntitySummary;

/// Home Assistant keeps a state within 255 characters. A longer one is cut,
/// so that a faulty answer cannot crowd the other entities out of the line.
const MAX_STATE_CHARS: usize = 255;
/// Longest name shown for an entity; a longer one is cut.
const MAX_NAME_CHARS: usize = 255;

/// Turns the states Home Assistant listed into the channel's entities. What
/// Home Assistant sends is data like any other: an entry whose id is not an
/// entity id is left out.
pub fn from_states(states: Vec<EntitySummary>) -> Vec<EntityInfo> {
    states
        .into_iter()
        .filter(|state| is_valid_entity_id(&state.entity_id))
        .map(|state| {
            let name = state
                .friendly_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map_or_else(
                    || state.entity_id.clone(),
                    |name| clip(name, MAX_NAME_CHARS),
                );
            EntityInfo {
                name,
                state: clip_state(&state.state),
                entity_id: state.entity_id,
            }
        })
        .collect()
}

/// A state as the channel passes it on.
pub fn clip_state(state: &str) -> String {
    clip(state, MAX_STATE_CHARS)
}

fn clip(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// The entities of the given domains, or all of them, ordered by entity id.
/// The order decides which entities are left out when the list does not fit
/// in one line: the last ones.
pub fn select(mut entities: Vec<EntityInfo>, domains: Option<&[String]>) -> Vec<EntityInfo> {
    if let Some(domains) = domains {
        entities.retain(|entity| {
            entity
                .entity_id
                .split_once('.')
                .is_some_and(|(domain, _)| domains.iter().any(|wanted| wanted == domain))
        });
    }
    entities.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
    entities
}

/// One entity as it appears in the answer to `entities`.
pub fn to_json(entity: &EntityInfo) -> Value {
    json!({
        "entity_id": entity.entity_id,
        "name": entity.name,
        "state": entity.state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(entity_id: &str, state: &str, friendly_name: Option<&str>) -> EntitySummary {
        EntitySummary {
            entity_id: entity_id.into(),
            state: state.into(),
            friendly_name: friendly_name.map(Into::into),
        }
    }

    fn entity(entity_id: &str) -> EntityInfo {
        EntityInfo {
            entity_id: entity_id.into(),
            name: entity_id.into(),
            state: "on".into(),
        }
    }

    fn ids(entities: &[EntityInfo]) -> Vec<&str> {
        entities
            .iter()
            .map(|entity| entity.entity_id.as_str())
            .collect()
    }

    #[test]
    fn the_name_is_the_friendly_name_or_the_id() {
        let entities = from_states(vec![
            summary("light.desk", "on", Some("Desk")),
            summary("light.shelf", "off", None),
            summary("light.floor", "off", Some("   ")),
        ]);
        assert_eq!(
            entities,
            [
                EntityInfo {
                    entity_id: "light.desk".into(),
                    name: "Desk".into(),
                    state: "on".into()
                },
                EntityInfo {
                    entity_id: "light.shelf".into(),
                    name: "light.shelf".into(),
                    state: "off".into()
                },
                EntityInfo {
                    entity_id: "light.floor".into(),
                    name: "light.floor".into(),
                    state: "off".into()
                },
            ]
        );
    }

    #[test]
    fn an_entry_that_is_not_an_entity_id_is_left_out() {
        let entities = from_states(vec![
            summary("light.desk", "on", None),
            summary("Light.Desk", "on", None),
            summary("light", "on", None),
            summary("light.desk\nother", "on", None),
            summary("", "on", None),
        ]);
        assert_eq!(ids(&entities), ["light.desk"]);
    }

    #[test]
    fn an_overlong_state_or_name_is_cut() {
        let entities = from_states(vec![summary(
            "sensor.text",
            &"s".repeat(1_000),
            Some(&"\u{142}".repeat(1_000)),
        )]);
        assert_eq!(entities[0].state.chars().count(), MAX_STATE_CHARS);
        assert_eq!(entities[0].name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn without_domains_every_entity_is_listed_in_order() {
        let entities = select(
            vec![
                entity("switch.fan"),
                entity("light.desk"),
                entity("scene.movie"),
            ],
            None,
        );
        assert_eq!(ids(&entities), ["light.desk", "scene.movie", "switch.fan"]);
    }

    #[test]
    fn domains_filter_the_list() {
        let all = vec![
            entity("switch.fan"),
            entity("light.desk"),
            entity("scene.movie"),
            entity("light_strip.shelf"),
        ];
        let domains = vec!["light".to_string(), "switch".to_string()];
        assert_eq!(
            ids(&select(all.clone(), Some(&domains))),
            ["light.desk", "switch.fan"]
        );
        assert!(select(all, Some(&[])).is_empty());
    }

    #[test]
    fn an_entity_is_written_as_in_the_reference() {
        assert_eq!(
            to_json(&EntityInfo {
                entity_id: "light.desk".into(),
                name: "Desk".into(),
                state: "on".into()
            }),
            json!({"entity_id": "light.desk", "name": "Desk", "state": "on"})
        );
    }
}
