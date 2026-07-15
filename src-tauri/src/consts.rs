//! All constant identifiers in one place (renaming the project = edit here).

pub const APP_NAME: &str = "Deskmate";
/// Config directory under %APPDATA%
pub const CONFIG_DIR: &str = "Deskmate";
/// Service name in Windows Credential Manager (MQTT password)
pub const KEYRING_SERVICE: &str = "Deskmate MQTT";
/// Toast AUMID: in release builds NSIS registers the identifier from tauri.conf.json
pub const TOAST_AUMID: &str = "com.deskmate.desktop";
/// Toast source label shown in the notification corner (branding)
pub const TOAST_DISPLAY_NAME: &str = "HomeOS";
/// MQTT topic prefix (base = "<prefix>/<node_id>")
pub const TOPIC_PREFIX: &str = "deskmate";
/// HA discovery prefix (standard)
pub const DISCOVERY_PREFIX: &str = "homeassistant";
/// URL scheme for toast button activation (click -> Windows launches deskmate:action?name=...)
pub const PROTOCOL_SCHEME: &str = "deskmate";

pub fn base_topic(node_id: &str) -> String {
    format!("{}/{}", TOPIC_PREFIX, node_id)
}
pub fn availability_topic(node_id: &str) -> String {
    format!("{}/availability", base_topic(node_id))
}
pub fn state_topic(node_id: &str, key: &str) -> String {
    format!("{}/state/{}", base_topic(node_id), key)
}
pub fn cmd_topic(node_id: &str, key: &str) -> String {
    format!("{}/cmd/{}", base_topic(node_id), key)
}
pub fn notify_topic(node_id: &str) -> String {
    format!("{}/notify", base_topic(node_id))
}
/// Deskmate PUBLISHES clicked toast actions here (an HA automation catches them).
pub fn notify_action_topic(node_id: &str) -> String {
    format!("{}/notify/action", base_topic(node_id))
}
