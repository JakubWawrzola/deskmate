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
