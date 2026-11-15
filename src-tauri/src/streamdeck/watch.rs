//! The watch list of one connection, and which `state` events a fresh read
//! of Home Assistant leads to.

use std::collections::HashMap;

/// The state of an entity Home Assistant does not have.
pub const UNAVAILABLE: &str = "unavailable";

/// The entities a connection watches and the state the client was last told
/// for each of them.
#[derive(Default)]
pub struct Watch {
    entities: Vec<String>,
    told: HashMap<String, String>,
    /// Counts the lists this connection has had. A read of Home Assistant
    /// that was started for an earlier list is recognised by it and dropped.
    generation: u64,
}

impl Watch {
    /// Replaces the list. The client has been told nothing about the new
    /// list yet. Returns the generation of the new list.
    pub fn replace(&mut self, entities: Vec<String>) -> u64 {
        self.entities = entities;
        self.told.clear();
        self.generation += 1;
        self.generation
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn entities(&self) -> &[String] {
        &self.entities
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Records what the answer to `watch` told the client. Ignored when the
    /// list has been replaced since that request.
    pub fn told(&mut self, generation: u64, states: &[(String, String)]) {
        if generation == self.generation {
            self.told = states.iter().cloned().collect();
        }
    }

    /// The entities whose state differs from what the client was last told,
    /// in the order of the list, and remembers them as told. Empty when the
    /// read belongs to an earlier list.
    pub fn changes(
        &mut self,
        generation: u64,
        read: &HashMap<String, String>,
    ) -> Vec<(String, String)> {
        if generation != self.generation {
            return Vec::new();
        }
        let mut changed = Vec::new();
        for (entity_id, state) in states_of(&self.entities, read) {
            if self.told.get(&entity_id) != Some(&state) {
                self.told.insert(entity_id.clone(), state.clone());
                changed.push((entity_id, state));
            }
        }
        changed
    }
}

/// One state for every entity of the list, in its order. An entity that the
/// read of Home Assistant does not contain is `unavailable`.
pub fn states_of(entities: &[String], read: &HashMap<String, String>) -> Vec<(String, String)> {
    entities
        .iter()
        .map(|entity_id| {
            let state = read
                .get(entity_id)
                .map_or_else(|| UNAVAILABLE.to_string(), Clone::clone);
            (entity_id.clone(), state)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn read(states: &[(&str, &str)]) -> HashMap<String, String> {
        states
            .iter()
            .map(|(id, state)| (id.to_string(), state.to_string()))
            .collect()
    }

    fn pairs(states: &[(&str, &str)]) -> Vec<(String, String)> {
        states
            .iter()
            .map(|(id, state)| (id.to_string(), state.to_string()))
            .collect()
    }

    #[test]
    fn every_entity_of_the_list_gets_a_state() {
        let entities = list(&["light.desk", "light.gone"]);
        assert_eq!(
            states_of(
                &entities,
                &read(&[("light.desk", "on"), ("light.other", "off")])
            ),
            pairs(&[("light.desk", "on"), ("light.gone", UNAVAILABLE)])
        );
    }

    #[test]
    fn only_a_changed_state_is_reported() {
        let mut watch = Watch::default();
        let generation = watch.replace(list(&["light.desk", "switch.fan"]));
        watch.told(
            generation,
            &pairs(&[("light.desk", "on"), ("switch.fan", "off")]),
        );

        let same = read(&[("light.desk", "on"), ("switch.fan", "off")]);
        assert!(watch.changes(generation, &same).is_empty());

        let changed = read(&[("light.desk", "off"), ("switch.fan", "off")]);
        assert_eq!(
            watch.changes(generation, &changed),
            pairs(&[("light.desk", "off")])
        );
        // Told once: the same read reports nothing more.
        assert!(watch.changes(generation, &changed).is_empty());
    }

    #[test]
    fn an_entity_that_disappears_becomes_unavailable_and_comes_back() {
        let mut watch = Watch::default();
        let generation = watch.replace(list(&["light.desk"]));
        watch.told(generation, &pairs(&[("light.desk", "on")]));

        assert_eq!(
            watch.changes(generation, &read(&[])),
            pairs(&[("light.desk", UNAVAILABLE)])
        );
        assert_eq!(
            watch.changes(generation, &read(&[("light.desk", "on")])),
            pairs(&[("light.desk", "on")])
        );
    }

    #[test]
    fn a_list_the_client_was_told_nothing_about_reports_every_entity() {
        let mut watch = Watch::default();
        let generation = watch.replace(list(&["light.desk", "switch.fan"]));
        assert_eq!(
            watch.changes(generation, &read(&[("switch.fan", "on")])),
            pairs(&[("light.desk", UNAVAILABLE), ("switch.fan", "on")])
        );
    }

    #[test]
    fn a_read_for_an_earlier_list_is_dropped() {
        let mut watch = Watch::default();
        let earlier = watch.replace(list(&["light.desk"]));
        let current = watch.replace(list(&["switch.fan"]));
        assert_ne!(earlier, current);

        assert!(watch
            .changes(earlier, &read(&[("light.desk", "on")]))
            .is_empty());
        watch.told(earlier, &pairs(&[("light.desk", "on")]));
        assert_eq!(
            watch.changes(current, &read(&[("switch.fan", "off")])),
            pairs(&[("switch.fan", "off")])
        );
    }

    #[test]
    fn replacing_the_list_forgets_what_was_told() {
        let mut watch = Watch::default();
        let first = watch.replace(list(&["light.desk"]));
        watch.told(first, &pairs(&[("light.desk", "on")]));
        let second = watch.replace(list(&["light.desk"]));
        assert_eq!(
            watch.changes(second, &read(&[("light.desk", "on")])),
            pairs(&[("light.desk", "on")])
        );
        assert!(!watch.is_empty());
        watch.replace(Vec::new());
        assert!(watch.is_empty());
        assert!(watch.entities().is_empty());
    }
}
