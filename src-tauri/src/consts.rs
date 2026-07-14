//! Wszystkie stale identyfikatory w jednym miejscu (rename projektu = edycja tutaj).

pub const APP_NAME: &str = "Deskmate";
/// Katalog configu w %APPDATA%
pub const CONFIG_DIR: &str = "Deskmate";
/// Usluga w Windows Credential Manager (haslo MQTT)
pub const KEYRING_SERVICE: &str = "Deskmate MQTT";
/// AUMID toastow: w release NSIS rejestruje identifier z tauri.conf.json
pub const TOAST_AUMID: &str = "com.deskmate.desktop";
/// Prefiks topicow MQTT (base = "<prefix>/<node_id>")
pub const TOPIC_PREFIX: &str = "deskmate";
/// Prefiks HA discovery (standard)
pub const DISCOVERY_PREFIX: &str = "homeassistant";

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
/// Deskmate PUBLIKUJE tu akcje klikniete w toascie (HA lapie automatyzacja).
pub fn notify_action_topic(node_id: &str) -> String {
    format!("{}/notify/action", base_topic(node_id))
}
