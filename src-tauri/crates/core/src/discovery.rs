//! Home Assistant MQTT Discovery - publishing entity configs (retained).
//! Format: https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
//! Also the entity declaration for the Deskmate Link integration.

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::config::AppConfig;
use crate::consts;
use crate::sensors::{OwnedSensorDef, SensorDef, BRIGHTNESS_KEY, SENSOR_DEFS};
use crate::sys_commands::{CommandDef, COMMAND_DEFS, PRESENT_DEFS};

fn device_block(cfg: &AppConfig) -> Value {
    json!({
        "identifiers": [format!("deskmate_{}", cfg.node_id)],
        "name": cfg.device_name,
        "manufacturer": "Deskmate",
        "model": consts::DEVICE_MODEL,
        "sw_version": env!("CARGO_PKG_VERSION"),
    })
}

/// (topic, payload retained; payload "" = remove entity from HA)
pub type DiscoveryMsg = (String, String);

/// Entities that only a client running in the user's session can describe.
/// The desktop app fills this in; a client without a signed-in user passes
/// the default, which declares none of them.
#[derive(Debug, Clone, Default)]
pub struct SessionEntities {
    /// Options of the select entities, by entity key (the sound devices). A
    /// select without options is not declared.
    pub select_options: HashMap<String, Vec<String>>,
    /// Descriptors of the family entities, in the form of a Link `declare`
    /// entry. Empty while the family features are off.
    pub family: Vec<Value>,
    /// Every family entity as (component, key), for removing them over MQTT.
    pub family_keys: &'static [(&'static str, &'static str)],
    /// Whether a Link declaration includes the media player: see
    /// `link::media::declares_media_player`. MQTT has no such entity.
    pub media_player: bool,
    /// A display has answered for its brightness since the app started.
    /// Until then the brightness number is not declared.
    pub brightness: bool,
}

impl SessionEntities {
    fn options_for(&self, key: &str) -> Vec<String> {
        self.select_options.get(key).cloned().unwrap_or_default()
    }

    /// A number entity this computer cannot offer right now.
    fn lacks_number(&self, key: &str) -> bool {
        key == BRIGHTNESS_KEY && !self.brightness
    }
}

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
pub fn build_all(
    cfg: &AppConfig,
    hardware_defs: &[OwnedSensorDef],
    session: &SessionEntities,
) -> Vec<DiscoveryMsg> {
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
                if !enabled || session.lacks_number(def.id) {
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
                if let Some(i) = def.icon {
                    p["icon"] = json!(i);
                }
                if let Some(u) = def.unit {
                    p["unit_of_measurement"] = json!(u);
                }
                out.push((topic, p.to_string()));
            }
            "select" => {
                // default sound devices; MQTT needs at least one option
                let topic = cfg_topic("select", node, object_id);
                let options = session.options_for(def.id);
                if !enabled || options.is_empty() {
                    out.push((topic, String::new()));
                    continue;
                }
                let mut p = json!({
                    "name": def.name,
                    "unique_id": format!("deskmate_{}_{}", node, def.id),
                    "state_topic": consts::state_topic(node, def.id),
                    "command_topic": consts::cmd_topic(node, def.id),
                    "options": options,
                    "availability_topic": avail,
                    "device": device,
                });
                if let Some(i) = def.icon {
                    p["icon"] = json!(i);
                }
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
                if let Some(u) = def.unit {
                    p["unit_of_measurement"] = json!(u);
                }
                if let Some(d) = def.device_class {
                    p["device_class"] = json!(d);
                }
                if let Some(i) = def.icon {
                    p["icon"] = json!(i);
                }
                if def.unit.is_some() && comp == "sensor" && def.device_class != Some("duration") {
                    p["state_class"] = json!("measurement");
                }
                out.push((topic, p.to_string()));
            }
        }
    }

    // Optional hardware sensors are included only after a successful local probe.
    for def in hardware_defs {
        let topic = cfg_topic("sensor", node, &def.id);
        if !crate::sensors::is_hardware_enabled(cfg, &def.id) {
            out.push((topic, String::new()));
            continue;
        }
        let mut payload = json!({
            "name": def.name,
            "unique_id": format!("deskmate_{}_{}", node, def.id),
            "state_topic": consts::state_topic(node, &def.id),
            "availability_topic": avail,
            "expire_after": expire,
            "device": device,
            "state_class": "measurement",
        });
        if let Some(unit) = &def.unit {
            payload["unit_of_measurement"] = json!(unit);
        }
        if let Some(device_class) = &def.device_class {
            payload["device_class"] = json!(device_class);
        }
        if let Some(icon) = &def.icon {
            payload["icon"] = json!(icon);
        }
        out.push((topic, payload.to_string()));
    }

    // --- sensors added on the Sensors page ---
    for sensor in &cfg.custom_sensors {
        let key = crate::custom_sensors::key(sensor);
        let component = crate::custom_sensors::component(sensor);
        let topic = cfg_topic(component, node, &key);
        if !sensor.enabled {
            out.push((topic, String::new()));
            continue;
        }
        let mut payload = json!({
            "name": sensor.name,
            "unique_id": format!("deskmate_{}_{}", node, key),
            "state_topic": consts::state_topic(node, &key),
            "availability_topic": avail,
            "expire_after": expire.max(sensor.interval_secs * 3),
            "icon": crate::custom_sensors::icon(sensor),
            "device": device,
        });
        if component == "binary_sensor" {
            payload["payload_on"] = json!("ON");
            payload["payload_off"] = json!("OFF");
            if sensor.kind == "process" {
                payload["device_class"] = json!("running");
            }
        } else if !sensor.unit.trim().is_empty() {
            payload["unit_of_measurement"] = json!(sensor.unit.trim());
        }
        out.push((topic, payload.to_string()));
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
            "switch" | "number" if c.action == "powershell" => c.kind.as_str(),
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
                "command_topic": cmd_t, "payload_press": "PRESS", "icon": command_icon(c),
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
    let remove =
        |comp: &str, key: &str| -> DiscoveryMsg { (cfg_topic(comp, node, key), String::new()) };

    // clipboard: text entity for setting it (when the clipboard bridge is enabled)
    if cfg.clipboard_write_mode != "off" {
        out.push(text_entity(
            "clipboard_set",
            "Set clipboard",
            "mdi:clipboard-arrow-down",
        ));
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
            })
            .to_string(),
        ));
    } else {
        out.push(remove("sensor", "clipboard"));
    }

    // text entry + presentation buttons (opt-in allow_input)
    if cfg.allow_input {
        out.push(text_entity(
            "type_text",
            "Type text",
            "mdi:keyboard-outline",
        ));
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

    // Mute: switch over the default playback device
    {
        let topic = cfg_topic("switch", node, "audio_mute");
        let p = json!({
            "name": "Mute audio",
            "unique_id": format!("deskmate_{}_audio_mute", node),
            "command_topic": consts::cmd_topic(node, "audio_mute"),
            "state_topic": consts::state_topic(node, "audio_mute"),
            "payload_on": "ON", "payload_off": "OFF",
            "icon": "mdi:volume-off",
            "availability_topic": avail,
            "device": device,
        });
        out.push((topic, p.to_string()));
    }

    // Microphone mute: switch over the default recording devices (audio.rs)
    {
        let topic = cfg_topic("switch", node, "mic_mute");
        let p = json!({
            "name": "Mute microphone",
            "unique_id": format!("deskmate_{}_mic_mute", node),
            "command_topic": consts::cmd_topic(node, "mic_mute"),
            "state_topic": consts::state_topic(node, "mic_mute"),
            "payload_on": "ON", "payload_off": "OFF",
            "icon": "mdi:microphone-off",
            "availability_topic": avail,
            "device": device,
        });
        out.push((topic, p.to_string()));
    }

    // Screen time and family rules (the app's family.rs): same entities as over Link
    if session.family.is_empty() {
        for (component, key) in session.family_keys {
            out.push(remove(component, key));
        }
    } else {
        for entity in &session.family {
            out.push(mqtt_from_descriptor(node, &avail, &device, entity));
        }
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
pub fn build_link_declare(
    cfg: &AppConfig,
    hardware_defs: &[OwnedSensorDef],
    session: &SessionEntities,
) -> Value {
    let mut entities = Vec::new();
    for def in SENSOR_DEFS {
        if !crate::sensors::is_enabled(cfg, def.id) || session.lacks_number(def.id) {
            continue;
        }
        let mut entity = link_sensor_entity(def);
        if def.component == "select" {
            // added once the first sensor tick has listed the devices
            let options = session.options_for(def.id);
            if options.is_empty() {
                continue;
            }
            entity["options"] = json!(options);
        }
        entities.push(entity);
    }
    for def in hardware_defs {
        if !crate::sensors::is_hardware_enabled(cfg, &def.id) {
            continue;
        }
        entities.push(link_hardware_entity(def));
    }
    for sensor in cfg.custom_sensors.iter().filter(|sensor| sensor.enabled) {
        let mut entity = json!({
            "key": crate::custom_sensors::key(sensor),
            "kind": crate::custom_sensors::component(sensor),
            "name": sensor.name,
            "icon": crate::custom_sensors::icon(sensor),
        });
        if sensor.kind == "process" {
            entity["device_class"] = json!("running");
        } else if !crate::custom_sensors::is_binary(&sensor.kind) && !sensor.unit.trim().is_empty()
        {
            entity["unit"] = json!(sensor.unit.trim());
        }
        entities.push(entity);
    }
    for def in COMMAND_DEFS {
        entities.push(link_command_entity(def));
    }
    for command in cfg.custom_commands.iter().filter(|command| command.enabled) {
        let kind = match command.kind.as_str() {
            "switch" | "number" if command.action == "powershell" => command.kind.as_str(),
            _ => "button",
        };
        let mut entity = json!({
            "key": format!("custom_{}", command.id),
            "kind": kind,
            "name": command.name,
            "icon": if kind == "switch" { "mdi:toggle-switch" } else if kind == "number" { "mdi:tune" } else { command_icon(command) },
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
            entities
                .push(json!({"key": def.id, "kind": "button", "name": def.name, "icon": def.icon}));
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
    entities.push(
        json!({"key": "keep_awake", "kind": "switch", "name": "Keep awake", "icon": "mdi:coffee"}),
    );
    entities.push(
        json!({"key": "audio_mute", "kind": "switch", "name": "Mute audio", "icon": "mdi:volume-off"}),
    );
    entities.push(
        json!({"key": "mic_mute", "kind": "switch", "name": "Mute microphone", "icon": "mdi:microphone-off"}),
    );
    entities.extend(session.family.iter().cloned());
    if session.media_player {
        entities.push(crate::link::media::media_entity());
    }
    for hotkey in &cfg.hotkeys {
        let display_name = if hotkey.name.is_empty() {
            &hotkey.id
        } else {
            &hotkey.name
        };
        entities.push(json!({
            "key": link_hotkey_key(&hotkey.id),
            "kind": "event",
            "name": format!("hotkey: {display_name}"),
            "event_types": ["press"],
        }));
    }

    json!({
        "t": "declare",
        "device": link_device(cfg),
        "entities": entities,
    })
}

/// The `device` block of a Link `declare`.
pub(crate) fn link_device(cfg: &AppConfig) -> Value {
    json!({
        "name": cfg.device_name,
        "model": consts::DEVICE_MODEL,
        "sw_version": env!("CARGO_PKG_VERSION"),
    })
}

/// Link descriptor of a built-in sensor. A select still needs its options.
pub(crate) fn link_sensor_entity(def: &SensorDef) -> Value {
    let mut entity = json!({
        "key": def.id,
        "kind": def.component,
        "name": def.name,
    });
    if let Some(unit) = def.unit {
        entity["unit"] = json!(unit);
    }
    if let Some(device_class) = def.device_class {
        entity["device_class"] = json!(device_class);
    }
    if let Some(icon) = def.icon {
        entity["icon"] = json!(icon);
    }
    if def.unit.is_some() && def.component == "sensor" && def.device_class != Some("duration") {
        entity["state_class"] = json!("measurement");
    }
    if def.component == "number" {
        entity["min"] = json!(0);
        entity["max"] = json!(100);
        entity["step"] = json!(1);
    }
    entity
}

/// Link descriptor of a hardware sensor detected on this computer.
pub(crate) fn link_hardware_entity(def: &OwnedSensorDef) -> Value {
    let mut entity = json!({
        "key": def.id,
        "kind": "sensor",
        "name": def.name,
        "state_class": "measurement",
    });
    if let Some(unit) = &def.unit {
        entity["unit"] = json!(unit);
    }
    if let Some(device_class) = &def.device_class {
        entity["device_class"] = json!(device_class);
    }
    if let Some(icon) = &def.icon {
        entity["icon"] = json!(icon);
    }
    entity
}

/// Link descriptor of a built-in command: a button.
pub(crate) fn link_command_entity(def: &CommandDef) -> Value {
    json!({"key": def.id, "kind": "button", "name": def.name, "icon": def.icon})
}

/// MQTT discovery for an entity described like a Link `declare` entry.
fn mqtt_from_descriptor(node: &str, avail: &str, device: &Value, entity: &Value) -> DiscoveryMsg {
    let kind = entity["kind"].as_str().unwrap_or("sensor");
    let key = entity["key"].as_str().unwrap_or_default();
    let mut p = json!({
        "name": entity["name"],
        "unique_id": format!("deskmate_{}_{}", node, key),
        "state_topic": consts::state_topic(node, key),
        "availability_topic": avail,
        "device": device,
    });
    if kind != "sensor" {
        p["command_topic"] = json!(consts::cmd_topic(node, key));
    }
    for (from, to) in [
        ("icon", "icon"),
        ("unit", "unit_of_measurement"),
        ("device_class", "device_class"),
        ("state_class", "state_class"),
        ("min", "min"),
        ("max", "max"),
        ("step", "step"),
        ("pattern", "pattern"),
        ("mode", "mode"),
        ("options", "options"),
    ] {
        if let Some(value) = entity.get(from) {
            p[to] = value.clone();
        }
    }
    if kind == "switch" {
        p["payload_on"] = json!("ON");
        p["payload_off"] = json!("OFF");
    }
    (cfg_topic(kind, node, key), p.to_string())
}

pub fn link_hotkey_key(hotkey_id: &str) -> String {
    format!("hotkey_{hotkey_id}")
}

pub fn remove_hardware(node_id: &str, id: &str) -> DiscoveryMsg {
    (cfg_topic("sensor", node_id, id), String::new())
}

/// Removal of a hotkey's device trigger (when deleting a hotkey from the UI).
pub fn remove_hotkey(node_id: &str, hotkey_id: &str) -> DiscoveryMsg {
    (
        cfg_topic("device_automation", node_id, &format!("hotkey_{hotkey_id}")),
        String::new(),
    )
}

/// Messages that remove a deleted custom sensor (both possible components).
pub fn remove_custom_sensor(node_id: &str, sensor_id: &str) -> Vec<DiscoveryMsg> {
    vec![
        (
            cfg_topic("binary_sensor", node_id, &format!("csb_{sensor_id}")),
            String::new(),
        ),
        (
            cfg_topic("sensor", node_id, &format!("cs_{sensor_id}")),
            String::new(),
        ),
    ]
}

/// Messages that remove the custom-control entities (all 3 components).
pub fn remove_custom(node_id: &str, custom_id: &str) -> Vec<DiscoveryMsg> {
    let object_id = format!("custom_{}", custom_id);
    ["button", "switch", "number"]
        .iter()
        .map(|comp| (cfg_topic(comp, node_id, &object_id), String::new()))
        .collect()
}

fn command_icon(command: &crate::config::CustomCommand) -> &'static str {
    match command.action.as_str() {
        "launch" => "mdi:play-box-outline",
        "stop" => "mdi:stop-circle-outline",
        "keys" => "mdi:keyboard-outline",
        _ => "mdi:console",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ActionSpec, Hotkey};
    use crate::sensors::OwnedSensorDef;

    fn entities(cfg: &AppConfig) -> Vec<Value> {
        build_link_declare(cfg, &[], &SessionEntities::default())["entities"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn descriptor<'a>(entities: &'a [Value], key: &str) -> Option<&'a Value> {
        entities.iter().find(|entity| entity["key"] == key)
    }

    #[test]
    fn the_device_model_names_the_platform() {
        #[cfg(windows)]
        let model = "Deskmate for Windows";
        #[cfg(target_os = "linux")]
        let model = "Deskmate for Linux";
        let cfg = AppConfig::default();
        assert_eq!(link_device(&cfg)["model"], model);
        assert_eq!(device_block(&cfg)["model"], model);
    }

    #[test]
    fn link_declare_adds_text_and_hotkey_event_with_mqtt_names() {
        let mut cfg = AppConfig {
            allow_input: true,
            tts_enabled: true,
            clipboard_write_mode: "automatic".into(),
            ..AppConfig::default()
        };
        cfg.hotkeys.push(Hotkey {
            id: "desk_lamp".into(),
            name: "Desk lamp".into(),
            accelerator: "Ctrl+Alt+L".into(),
            action: ActionSpec {
                kind: "mqtt".into(),
                ..Default::default()
            },
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
    fn the_media_player_is_declared_only_when_the_session_says_so() {
        let cfg = AppConfig::default();
        assert!(descriptor(&entities(&cfg), "media").is_none());
        let session = SessionEntities {
            media_player: true,
            ..SessionEntities::default()
        };
        let declared = build_link_declare(&cfg, &[], &session)["entities"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(
            descriptor(&declared, "media"),
            Some(&json!({"key": "media", "kind": "media_player", "name": "Media"}))
        );
        // MQTT has no media player.
        assert!(build_all(&cfg, &[], &session)
            .iter()
            .all(|(topic, _)| !topic.contains("/media_player/")));
    }

    #[test]
    fn smaller_link_declare_omits_disabled_dynamic_entities() {
        let mut cfg = AppConfig {
            allow_input: true,
            ..AppConfig::default()
        };
        cfg.hotkeys.push(Hotkey {
            id: "temporary".into(),
            name: String::new(),
            accelerator: String::new(),
            action: ActionSpec::default(),
        });
        let before = entities(&cfg);
        assert!(descriptor(&before, "type_text").is_some());
        assert_eq!(
            descriptor(&before, "hotkey_temporary").unwrap()["name"],
            "hotkey: temporary"
        );

        cfg.allow_input = false;
        cfg.hotkeys.clear();
        let after = entities(&cfg);
        assert!(descriptor(&after, "type_text").is_none());
        assert!(descriptor(&after, "hotkey_temporary").is_none());
    }

    #[test]
    fn brightness_is_declared_only_while_a_display_answers_and_it_is_on() {
        let with_display = SessionEntities {
            brightness: true,
            ..SessionEntities::default()
        };
        let mut cfg = AppConfig::default();

        let declared = build_link_declare(&cfg, &[], &with_display);
        let listed = declared["entities"].as_array().unwrap();
        let brightness = descriptor(listed, BRIGHTNESS_KEY).unwrap();
        assert_eq!(brightness["kind"], "number");
        assert_eq!(brightness["unit"], "%");
        assert_eq!(
            (brightness["min"].clone(), brightness["max"].clone()),
            (json!(0), json!(100))
        );
        assert!(
            descriptor(&entities(&cfg), BRIGHTNESS_KEY).is_none(),
            "no display"
        );

        cfg.sensors_enabled.insert(BRIGHTNESS_KEY.into(), false);
        let switched_off = build_link_declare(&cfg, &[], &with_display);
        assert!(descriptor(switched_off["entities"].as_array().unwrap(), BRIGHTNESS_KEY).is_none());
    }

    #[test]
    fn mqtt_removes_brightness_without_a_display() {
        let cfg = AppConfig::default();
        let topic = cfg_topic("number", &cfg.node_id, BRIGHTNESS_KEY);
        let payload_for = |session: &SessionEntities| {
            build_all(&cfg, &[], session)
                .into_iter()
                .find(|(candidate, _)| *candidate == topic)
                .map(|(_, payload)| payload)
        };
        assert_eq!(
            payload_for(&SessionEntities::default()),
            Some(String::new())
        );
        let declared = payload_for(&SessionEntities {
            brightness: true,
            ..SessionEntities::default()
        })
        .unwrap();
        assert!(declared.contains("\"command_topic\""), "{declared}");
    }

    #[test]
    fn screen_on_is_declared_by_default() {
        let declared = entities(&AppConfig::default());
        assert_eq!(
            descriptor(&declared, "screen_on").unwrap()["kind"],
            "binary_sensor"
        );
    }

    #[test]
    fn user_state_is_privacy_sensitive_and_opt_in() {
        let def = SENSOR_DEFS
            .iter()
            .find(|def| def.id == "user_state")
            .unwrap();
        assert!(def.privacy && !def.default_enabled);
        let mut cfg = AppConfig::default();
        assert!(descriptor(&entities(&cfg), "user_state").is_none());

        cfg.sensors_enabled.insert("user_state".into(), true);
        assert_eq!(
            descriptor(&entities(&cfg), "user_state").unwrap()["kind"],
            "sensor"
        );
    }

    #[test]
    fn wake_screen_is_declared_only_with_allow_input() {
        let mut cfg = AppConfig::default();
        assert!(descriptor(&entities(&cfg), "wake_screen").is_none());
        let mqtt_payload = |cfg: &AppConfig| {
            let topic = cfg_topic("button", &cfg.node_id, "wake_screen");
            build_all(cfg, &[], &SessionEntities::default())
                .into_iter()
                .find(|(candidate, _)| *candidate == topic)
                .map(|(_, payload)| payload)
        };
        assert_eq!(mqtt_payload(&cfg), Some(String::new()), "removed");

        cfg.allow_input = true;
        assert_eq!(
            descriptor(&entities(&cfg), "wake_screen").unwrap()["kind"],
            "button"
        );
        let declared = mqtt_payload(&cfg).unwrap();
        assert!(declared.contains("\"command_topic\""), "{declared}");
    }

    #[test]
    fn hardware_sensor_is_declared_only_when_detected_and_enabled() {
        let mut cfg = AppConfig::default();
        let hardware = vec![OwnedSensorDef {
            id: "gpu_usage".into(),
            name: "GPU usage".into(),
            component: "sensor".into(),
            unit: Some("%".into()),
            device_class: None,
            icon: Some("mdi:expansion-card".into()),
            privacy: false,
            default_enabled: true,
            hardware: true,
        }];
        let declared = build_link_declare(&cfg, &hardware, &SessionEntities::default());
        assert!(declared["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entity| entity["key"] == "gpu_usage"));

        cfg.sensors_enabled.insert("gpu_usage".into(), false);
        let disabled = build_link_declare(&cfg, &hardware, &SessionEntities::default());
        assert!(!disabled["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entity| entity["key"] == "gpu_usage"));

        let unavailable =
            build_link_declare(&AppConfig::default(), &[], &SessionEntities::default());
        assert!(!unavailable["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entity| entity["key"] == "gpu_usage"));
    }
}
