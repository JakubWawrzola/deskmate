//! Home Assistant MQTT Discovery - publishing entity configs (retained).
//! Format: https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery

use serde_json::{json, Value};

use crate::config::AppConfig;
use crate::consts;
use crate::sensors::SENSOR_DEFS;
use crate::sys_commands::{COMMAND_DEFS, PRESENT_DEFS};

fn device_block(cfg: &AppConfig) -> Value {
    json!({
        "identifiers": [format!("deskmate_{}", cfg.node_id)],
        "name": cfg.device_name,
        "manufacturer": "Deskmate",
        "model": "Deskmate for Windows",
        "sw_version": env!("CARGO_PKG_VERSION"),
    })
}

/// (topic, payload retained; payload "" = remove entity from HA)
pub type DiscoveryMsg = (String, String);

fn cfg_topic(component: &str, node_id: &str, object_id: &str) -> String {
    format!(
        "{}/{}/{}/{}/config",
        consts::DISCOVERY_PREFIX,
        component,
        node_id,
        object_id
    )
}

/// Full set of discovery messages derived from the config.
/// Disabled entities get an empty payload (removal from HA).
pub fn build_all(cfg: &AppConfig) -> Vec<DiscoveryMsg> {
    let mut out = Vec::new();
    let node = &cfg.node_id;
    let avail = consts::availability_topic(node);
    let device = device_block(cfg);
    let expire = (cfg.publish_interval_secs * 4).max(60);

    // --- sensors / binary_sensors / number(volume) ---
    for def in SENSOR_DEFS {
        let enabled = crate::sensors::is_enabled(cfg, def.id);
        let object_id = def.id;
        match def.component {
            "number" => {
                // volume: number with command_topic
                let topic = cfg_topic("number", node, object_id);
                if !enabled {
                    out.push((topic, String::new()));
                    continue;
                }
                let mut p = json!({
                    "name": def.name,
                    "unique_id": format!("deskmate_{}_{}", node, def.id),
                    "state_topic": consts::state_topic(node, def.id),
                    "command_topic": consts::cmd_topic(node, def.id),
                    "min": 0, "max": 100, "step": 1,
                    "availability_topic": avail,
                    "device": device,
                });
                if let Some(i) = def.icon { p["icon"] = json!(i); }
                if let Some(u) = def.unit { p["unit_of_measurement"] = json!(u); }
                out.push((topic, p.to_string()));
            }
            comp => {
                let topic = cfg_topic(comp, node, object_id);
                if !enabled {
                    out.push((topic, String::new()));
                    continue;
                }
                let mut p = json!({
                    "name": def.name,
                    "unique_id": format!("deskmate_{}_{}", node, def.id),
                    "state_topic": consts::state_topic(node, def.id),
                    "availability_topic": avail,
                    "device": device,
                });
                if comp == "sensor" || comp == "binary_sensor" {
                    p["expire_after"] = json!(expire);
                }
                if let Some(u) = def.unit { p["unit_of_measurement"] = json!(u); }
                if let Some(d) = def.device_class { p["device_class"] = json!(d); }
                if let Some(i) = def.icon { p["icon"] = json!(i); }
                if def.unit.is_some() && comp == "sensor" && def.device_class != Some("duration") {
                    p["state_class"] = json!("measurement");
                }
                out.push((topic, p.to_string()));
            }
        }
    }

    // --- predefined commands (buttons) ---
    for def in COMMAND_DEFS {
        let topic = cfg_topic("button", node, def.id);
        let p = json!({
            "name": def.name,
            "unique_id": format!("deskmate_{}_{}", node, def.id),
            "command_topic": consts::cmd_topic(node, def.id),
            "payload_press": "PRESS",
            "icon": def.icon,
            "availability_topic": avail,
            "device": device,
        });
        out.push((topic, p.to_string()));
    }

    // --- custom user controls (button | switch | number) ---
    for c in &cfg.custom_commands {
        let object_id = format!("custom_{}", c.id);
        if !c.enabled {
            for comp in ["button", "switch", "number"] {
                out.push((cfg_topic(comp, node, &object_id), String::new()));
            }
            continue;
        }
        let cmd_t = consts::cmd_topic(node, &format!("custom_{}", c.id));
        let uid = format!("deskmate_{}_custom_{}", node, c.id);
        // clear the other components (in case the control type was changed)
        let active = match c.kind.as_str() {
            "switch" | "number" => c.kind.as_str(),
            _ => "button",
        };
        for comp in ["button", "switch", "number"] {
            if comp != active {
                out.push((cfg_topic(comp, node, &object_id), String::new()));
            }
        }
        let p = match active {
            "switch" => json!({
                "name": c.name, "unique_id": uid,
                "command_topic": cmd_t, "payload_on": "ON", "payload_off": "OFF",
                "optimistic": true, "icon": "mdi:toggle-switch",
                "availability_topic": avail, "device": device,
            }),
            "number" => json!({
                "name": c.name, "unique_id": uid,
                "command_topic": cmd_t,
                "min": c.num_min, "max": c.num_max, "step": c.num_step,
                "mode": "box", "optimistic": true, "icon": "mdi:tune",
                "availability_topic": avail, "device": device,
            }),
            _ => json!({
                "name": c.name, "unique_id": uid,
                "command_topic": cmd_t, "payload_press": "PRESS", "icon": "mdi:console",
                "availability_topic": avail, "device": device,
            }),
        };
        out.push((cfg_topic(active, node, &object_id), p.to_string()));
    }

    // --- text entities (HA -> PC) + presentation + TTS, per opt-in ---
    let text_entity = |key: &str, name: &str, icon: &str| -> DiscoveryMsg {
        (
            cfg_topic("text", node, key),
            json!({
                "name": name,
                "unique_id": format!("deskmate_{}_{}", node, key),
                "command_topic": consts::cmd_topic(node, key),
                "icon": icon,
                "availability_topic": avail,
                "device": device,
            })
            .to_string(),
        )
    };
    let remove = |comp: &str, key: &str| -> DiscoveryMsg { (cfg_topic(comp, node, key), String::new()) };

    // clipboard: text entity for setting it (when the clipboard bridge is enabled)
    if cfg.clipboard_write_mode != "off" {
        out.push(text_entity("clipboard_set", "Set clipboard", "mdi:clipboard-arrow-down"));
    } else {
        out.push(remove("text", "clipboard_set"));
    }

    // clipboard read sensor is controlled independently from clipboard writes
    if cfg.clipboard_read_mode != "off" {
        let topic = cfg_topic("sensor", node, "clipboard");
        out.push((
            topic,
            json!({
                "name": "Clipboard",
                "unique_id": format!("deskmate_{}_clipboard", node),
                "state_topic": consts::state_topic(node, "clipboard"),
                "icon": "mdi:clipboard-text",
                "expire_after": expire,
                "availability_topic": avail,
                "device": device,
            }).to_string(),
        ));
    } else {
        out.push(remove("sensor", "clipboard"));
    }

    // text entry + presentation buttons (opt-in allow_input)
    if cfg.allow_input {
        out.push(text_entity("type_text", "Type text", "mdi:keyboard-outline"));
        for def in PRESENT_DEFS {
            out.push((
                cfg_topic("button", node, def.id),
                json!({
                    "name": def.name,
                    "unique_id": format!("deskmate_{}_{}", node, def.id),
                    "command_topic": consts::cmd_topic(node, def.id),
                    "payload_press": "PRESS",
                    "icon": def.icon,
                    "availability_topic": avail,
                    "device": device,
                })
                .to_string(),
            ));
        }
    } else {
        out.push(remove("text", "type_text"));
        for def in PRESENT_DEFS {
            out.push(remove("button", def.id));
        }
    }

    // TTS: text entity (opt-in tts_enabled)
    if cfg.tts_enabled {
        out.push(text_entity("tts_say", "Say (TTS)", "mdi:account-voice"));
    } else {
        out.push(remove("text", "tts_say"));
    }

    // open URL: text entity (same trust group as text entry)
    if cfg.allow_input {
        out.push(text_entity("open_url", "Open URL", "mdi:open-in-new"));
    } else {
        out.push(remove("text", "open_url"));
    }

    // Keep awake: switch (blocks sleep/screen-off) - always available
    {
        let topic = cfg_topic("switch", node, "keep_awake");
        let p = json!({
            "name": "Keep awake",
            "unique_id": format!("deskmate_{}_keep_awake", node),
            "command_topic": consts::cmd_topic(node, "keep_awake"),
            "state_topic": consts::state_topic(node, "keep_awake"),
            "payload_on": "ON", "payload_off": "OFF",
            "icon": "mdi:coffee",
            "availability_topic": avail,
            "device": device,
        });
        out.push((topic, p.to_string()));
    }

    // Hotkeys of type mqtt -> device triggers (HA automations without an API token)
    for h in &cfg.hotkeys {
        let topic = cfg_topic("device_automation", node, &format!("hotkey_{}", h.id));
        if h.action.kind == "mqtt" {
            let p = json!({
                "automation_type": "trigger",
                "type": "button_short_press",
                "subtype": format!("hotkey: {}", if h.name.is_empty() { &h.id } else { &h.name }),
                "topic": format!("{}/hotkey/{}", consts::base_topic(node), h.id),
                "payload": "PRESS",
                "device": device,
            });
            out.push((topic, p.to_string()));
        } else {
            out.push((topic, String::new()));
        }
    }

    out
}

/// Dynamic entity declaration for the Deskmate Link integration.
pub fn build_link_declare(cfg: &AppConfig) -> Value {
    let mut entities = Vec::new();
    for def in SENSOR_DEFS {
        if !crate::sensors::is_enabled(cfg, def.id) {
            continue;
        }
        let mut entity = json!({
            "key": def.id,
            "kind": def.component,
            "name": def.name,
        });
        if let Some(unit) = def.unit { entity["unit"] = json!(unit); }
        if let Some(device_class) = def.device_class { entity["device_class"] = json!(device_class); }
        if let Some(icon) = def.icon { entity["icon"] = json!(icon); }
        if def.unit.is_some() && def.component == "sensor" && def.device_class != Some("duration") {
            entity["state_class"] = json!("measurement");
        }
        if def.component == "number" {
            entity["min"] = json!(0);
            entity["max"] = json!(100);
            entity["step"] = json!(1);
        }
        entities.push(entity);
    }
    for def in COMMAND_DEFS {
        entities.push(json!({"key": def.id, "kind": "button", "name": def.name, "icon": def.icon}));
    }
    for command in cfg.custom_commands.iter().filter(|command| command.enabled) {
        let kind = match command.kind.as_str() {
            "switch" | "number" => command.kind.as_str(),
            _ => "button",
        };
        let mut entity = json!({
            "key": format!("custom_{}", command.id),
            "kind": kind,
            "name": command.name,
            "icon": if kind == "switch" { "mdi:toggle-switch" } else if kind == "number" { "mdi:tune" } else { "mdi:console" },
        });
        if kind == "number" {
            entity["min"] = json!(command.num_min);
            entity["max"] = json!(command.num_max);
            entity["step"] = json!(command.num_step);
        }
        entities.push(entity);
    }
    if cfg.allow_input {
        entities.push(json!({
            "key": "type_text",
            "kind": "text",
            "name": "Type text",
            "icon": "mdi:keyboard-outline",
            "mode": "text",
        }));
        for def in PRESENT_DEFS {
            entities.push(json!({"key": def.id, "kind": "button", "name": def.name, "icon": def.icon}));
        }
        entities.push(json!({
            "key": "open_url",
            "kind": "text",
            "name": "Open URL",
            "icon": "mdi:open-in-new",
            "mode": "text",
        }));
    }
    if cfg.tts_enabled {
        entities.push(json!({
            "key": "tts_say",
            "kind": "text",
            "name": "Say (TTS)",
            "icon": "mdi:account-voice",
            "mode": "text",
        }));
    }
    if cfg.clipboard_write_mode != "off" {
        entities.push(json!({
            "key": "clipboard_set",
            "kind": "text",
            "name": "Set clipboard",
            "icon": "mdi:clipboard-arrow-down",
            "mode": "text",
        }));
    }
    if cfg.clipboard_read_mode != "off" {
        entities.push(json!({"key": "clipboard", "kind": "sensor", "name": "Clipboard", "icon": "mdi:clipboard-text"}));
    }
    entities.push(json!({"key": "keep_awake", "kind": "switch", "name": "Keep awake", "icon": "mdi:coffee"}));
    for hotkey in &cfg.hotkeys {
        let display_name = if hotkey.name.is_empty() { &hotkey.id } else { &hotkey.name };
        entities.push(json!({
            "key": link_hotkey_key(&hotkey.id),
            "kind": "event",
            "name": format!("hotkey: {display_name}"),
            "event_types": ["press"],
        }));
    }

    json!({
        "t": "declare",
        "device": {
            "name": cfg.device_name,
            "model": "Deskmate for Windows",
            "sw_version": env!("CARGO_PKG_VERSION"),
        },
        "entities": entities,
    })
}

pub fn link_hotkey_key(hotkey_id: &str) -> String {
    format!("hotkey_{hotkey_id}")
}

/// Removal of a hotkey's device trigger (when deleting a hotkey from the UI).
pub fn remove_hotkey(node_id: &str, hotkey_id: &str) -> DiscoveryMsg {
    (cfg_topic("device_automation", node_id, &format!("hotkey_{hotkey_id}")), String::new())
}

/// Messages that remove the custom-control entities (all 3 components).
pub fn remove_custom(node_id: &str, custom_id: &str) -> Vec<DiscoveryMsg> {
    let object_id = format!("custom_{}", custom_id);
    ["button", "switch", "number"]
        .iter()
        .map(|comp| (cfg_topic(comp, node_id, &object_id), String::new()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ActionSpec, Hotkey};

    fn entities(cfg: &AppConfig) -> Vec<Value> {
        build_link_declare(cfg)["entities"].as_array().unwrap().clone()
    }

    fn descriptor<'a>(entities: &'a [Value], key: &str) -> Option<&'a Value> {
        entities.iter().find(|entity| entity["key"] == key)
    }

    #[test]
    fn link_declare_adds_text_and_hotkey_event_with_mqtt_names() {
        let mut cfg = AppConfig::default();
        cfg.allow_input = true;
        cfg.tts_enabled = true;
        cfg.clipboard_write_mode = "automatic".into();
        cfg.hotkeys.push(Hotkey {
            id: "desk_lamp".into(),
            name: "Desk lamp".into(),
            accelerator: "Ctrl+Alt+L".into(),
            action: ActionSpec { kind: "mqtt".into(), ..Default::default() },
        });
        let declared = entities(&cfg);

        for (key, name) in [
            ("type_text", "Type text"),
            ("open_url", "Open URL"),
            ("tts_say", "Say (TTS)"),
            ("clipboard_set", "Set clipboard"),
        ] {
            let entity = descriptor(&declared, key).unwrap();
            assert_eq!(entity["kind"], "text");
            assert_eq!(entity["name"], name);
            assert_eq!(entity["mode"], "text");
        }
        let event = descriptor(&declared, "hotkey_desk_lamp").unwrap();
        assert_eq!(event["kind"], "event");
        assert_eq!(event["name"], "hotkey: Desk lamp");
        assert_eq!(event["event_types"], json!(["press"]));
    }

    #[test]
    fn smaller_link_declare_omits_disabled_dynamic_entities() {
        let mut cfg = AppConfig::default();
        cfg.allow_input = true;
        cfg.hotkeys.push(Hotkey {
            id: "temporary".into(),
            name: String::new(),
            accelerator: String::new(),
            action: ActionSpec::default(),
        });
        let before = entities(&cfg);
        assert!(descriptor(&before, "type_text").is_some());
        assert_eq!(descriptor(&before, "hotkey_temporary").unwrap()["name"], "hotkey: temporary");

        cfg.allow_input = false;
        cfg.hotkeys.clear();
        let after = entities(&cfg);
        assert!(descriptor(&after, "type_text").is_none());
        assert!(descriptor(&after, "hotkey_temporary").is_none());
    }
}
