//! Home Assistant MQTT Discovery - publikacja configow encji (retained).
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

/// (topic, payload retained; payload "" = usun encje z HA)
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

/// Pelny zestaw wiadomosci discovery wynikajacy z configu.
/// Encje wylaczone dostaja pusty payload (usuniecie z HA).
pub fn build_all(cfg: &AppConfig) -> Vec<DiscoveryMsg> {
    let mut out = Vec::new();
    let node = &cfg.node_id;
    let avail = consts::availability_topic(node);
    let device = device_block(cfg);
    let expire = (cfg.publish_interval_secs * 4).max(60);

    // --- sensory / binary_sensory / number(volume) ---
    for def in SENSOR_DEFS {
        let enabled = crate::sensors::is_enabled(cfg, def.id);
        let object_id = def.id;
        match def.component {
            "number" => {
                // volume: number z command_topic
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

    // --- komendy predefiniowane (buttony) ---
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

    // --- custom kontrolki uzytkownika (button | switch | number) ---
    for c in &cfg.custom_commands {
        let object_id = format!("custom_{}", c.id);
        let cmd_t = consts::cmd_topic(node, &format!("custom_{}", c.id));
        let uid = format!("deskmate_{}_custom_{}", node, c.id);
        // wyczysc pozostale komponenty (gdyby zmieniono typ kontrolki)
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

    // --- encje text (HA -> PC) + prezentacja + TTS, wg opt-in ---
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

    // schowek: encja text do ustawiania (gdy bridge schowka wlaczony)
    if crate::sensors::is_enabled(cfg, "clipboard") {
        out.push(text_entity("clipboard_set", "Set clipboard", "mdi:clipboard-arrow-down"));
    } else {
        out.push(remove("text", "clipboard_set"));
    }

    // wpis tekstu + przyciski prezentacji (opt-in allow_input)
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

    // TTS: encja text (opt-in tts_enabled)
    if cfg.tts_enabled {
        out.push(text_entity("tts_say", "Say (TTS)", "mdi:account-voice"));
    } else {
        out.push(remove("text", "tts_say"));
    }

    out
}

/// Wiadomosci usuwajace encje custom-kontrolki (wszystkie 3 komponenty).
pub fn remove_custom(node_id: &str, custom_id: &str) -> Vec<DiscoveryMsg> {
    let object_id = format!("custom_{}", custom_id);
    ["button", "switch", "number"]
        .iter()
        .map(|comp| (cfg_topic(comp, node_id, &object_id), String::new()))
        .collect()
}
