//! The volume of single programs: for each program the user listed, a volume
//! slider and a mute switch in Home Assistant, the same two controls the
//! Windows volume mixer has. This module names the entities and describes
//! them; reading and setting the levels is up to the app.
//!
//! Which programs run and play sound is something the user did not ask to
//! share, so nothing is enumerated for Home Assistant: only the programs on
//! the list get entities, and the list is empty until the user fills it.

use serde_json::{json, Value};

use crate::config::sanitize_id;

/// Most programs a user may list.
pub const MAX_PROGRAMS: usize = 16;
/// Entity keys: `appvol_discord` and `appmute_discord` for `discord.exe`.
pub const VOLUME_PREFIX: &str = "appvol_";
pub const MUTE_PREFIX: &str = "appmute_";
/// What a level is sent as while the program has no sound session; Home
/// Assistant shows the entity as unknown then.
pub const NO_SESSION: &str = "None";

/// Which of the two controls of a program a key names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Volume,
    Mute,
}

/// "discord.exe" -> "discord": the part of the entity keys that names the
/// program.
pub fn slug(program: &str) -> String {
    let name = program.trim().to_lowercase();
    sanitize_id(name.strip_suffix(".exe").unwrap_or(&name))
}

pub fn volume_key(program: &str) -> String {
    format!("{VOLUME_PREFIX}{}", slug(program))
}

pub fn mute_key(program: &str) -> String {
    format!("{MUTE_PREFIX}{}", slug(program))
}

/// The program on the list that `key` belongs to, and which of its controls.
/// A key for a program that is not listed belongs to nothing, so Home
/// Assistant can change no program the user did not pick.
pub fn program_for<'a>(programs: &'a [String], key: &str) -> Option<(&'a str, Control)> {
    let (wanted, control) = if let Some(rest) = key.strip_prefix(VOLUME_PREFIX) {
        (rest, Control::Volume)
    } else {
        (key.strip_prefix(MUTE_PREFIX)?, Control::Mute)
    };
    programs
        .iter()
        .find(|program| slug(program) == wanted)
        .map(|program| (program.as_str(), control))
}

/// The list as it is stored: one entry per program, none that would share
/// its entity keys with another, at most `MAX_PROGRAMS`.
pub fn normalize(programs: Vec<String>) -> Result<Vec<String>, String> {
    let mut kept: Vec<String> = Vec::new();
    for program in programs {
        if !kept.iter().any(|other| slug(other) == slug(&program)) {
            kept.push(program);
        }
    }
    if kept.len() > MAX_PROGRAMS {
        return Err(format!("at most {MAX_PROGRAMS} programs"));
    }
    Ok(kept)
}

/// The name of a program in an entity name: "Discord" for "discord.exe".
fn label(program: &str) -> String {
    let name = program.trim();
    let name = name
        .strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name);
    let mut letters = name.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// The descriptors of a Link `declare` for every listed program.
pub fn link_entities(programs: &[String]) -> Vec<Value> {
    programs
        .iter()
        .flat_map(|program| {
            let name = label(program);
            [
                json!({
                    "key": volume_key(program),
                    "kind": "number",
                    "name": format!("{name} volume"),
                    "icon": "mdi:volume-high",
                    "unit": "%",
                    "min": 0,
                    "max": 100,
                    "step": 1,
                }),
                json!({
                    "key": mute_key(program),
                    "kind": "switch",
                    "name": format!("Mute {name}"),
                    "icon": "mdi:volume-off",
                }),
            ]
        })
        .collect()
}

/// A volume from Home Assistant as whole percent; none for anything that is
/// not a number from 0 to 100.
pub fn parse_percent(payload: &str) -> Option<u8> {
    let value: f64 = payload.trim().parse().ok()?;
    (value.is_finite() && (0.0..=100.0).contains(&value)).then(|| value.round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed() -> Vec<String> {
        vec!["discord.exe".into(), "eldenring.exe".into()]
    }

    #[test]
    fn a_program_gets_a_volume_key_and_a_mute_key() {
        assert_eq!(volume_key("discord.exe"), "appvol_discord");
        assert_eq!(mute_key("discord.exe"), "appmute_discord");
        assert_eq!(volume_key(" My-Game.EXE "), "appvol_my_game");
        assert_eq!(slug("vlc"), "vlc");
    }

    #[test]
    fn a_key_finds_its_program_only_on_the_list() {
        let programs = listed();
        assert_eq!(
            program_for(&programs, "appvol_discord"),
            Some(("discord.exe", Control::Volume))
        );
        assert_eq!(
            program_for(&programs, "appmute_eldenring"),
            Some(("eldenring.exe", Control::Mute))
        );
        for key in [
            "appvol_chrome",
            "appmute_",
            "appvol_",
            "volume",
            "audio_mute",
            "appvol_discord.exe",
            "",
        ] {
            assert_eq!(program_for(&programs, key), None, "{key}");
        }
        assert_eq!(program_for(&[], "appvol_discord"), None);
    }

    #[test]
    fn the_list_keeps_one_entry_per_set_of_keys() {
        let kept = normalize(vec![
            "discord.exe".into(),
            "Discord.exe".into(),
            "my-app.exe".into(),
            "my_app.exe".into(),
        ])
        .unwrap();
        assert_eq!(kept, ["discord.exe", "my-app.exe"]);

        let many = (0..=MAX_PROGRAMS).map(|n| format!("app{n}.exe")).collect();
        assert!(normalize(many).is_err());
        assert_eq!(normalize(Vec::new()).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn declares_a_slider_and_a_switch_per_program() {
        let entities = link_entities(&["discord.exe".into()]);
        assert_eq!(
            entities,
            [
                json!({"key": "appvol_discord", "kind": "number", "name": "Discord volume",
                       "icon": "mdi:volume-high", "unit": "%", "min": 0, "max": 100, "step": 1}),
                json!({"key": "appmute_discord", "kind": "switch", "name": "Mute Discord",
                       "icon": "mdi:volume-off"}),
            ]
        );
        assert!(link_entities(&[]).is_empty());
    }

    #[test]
    fn a_volume_is_a_number_from_0_to_100() {
        assert_eq!(parse_percent("0"), Some(0));
        assert_eq!(parse_percent(" 42.4 "), Some(42));
        assert_eq!(parse_percent("99.5"), Some(100));
        assert_eq!(parse_percent("100"), Some(100));
        for bad in ["", "loud", "-1", "100.5", "101", "NaN", "inf", "1e9"] {
            assert_eq!(parse_percent(bad), None, "{bad}");
        }
    }
}
