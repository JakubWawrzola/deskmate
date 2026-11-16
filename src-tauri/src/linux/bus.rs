//! D-Bus: the session bus of the signed-in user (notifications, media
//! players, the tray host, the keyring) and the system bus (logind).
//!
//! Every call has a short timeout, so a bus that hangs costs one sensor
//! value, not the sensor loop. A connection is opened per use: the callers
//! run every few seconds at most, and nothing stays connected that could go
//! stale when the desktop restarts a service.

use dbus::blocking::{Connection, Proxy};
use std::time::Duration;

/// How long one call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_millis(1500);

const BUS_NAME: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// The session bus of the user this app runs for.
pub fn session() -> Result<Connection, String> {
    Connection::new_session().map_err(|e| format!("no session bus: {e}"))
}

/// The system bus, where logind is.
pub fn system() -> Result<Connection, String> {
    Connection::new_system().map_err(|e| format!("no system bus: {e}"))
}

fn bus_proxy(connection: &Connection) -> Proxy<'_, &Connection> {
    connection.with_proxy(BUS_NAME, BUS_PATH, CALL_TIMEOUT)
}

/// Whether a program has taken `name` right now.
pub fn name_has_owner(connection: &Connection, name: &str) -> bool {
    bus_proxy(connection)
        .method_call::<(bool,), _, _, _>(BUS_NAME, "NameHasOwner", (name,))
        .map(|(owned,)| owned)
        .unwrap_or(false)
}

/// Whether the bus can start a program for `name` when it is called.
pub fn name_is_activatable(connection: &Connection, name: &str) -> bool {
    bus_proxy(connection)
        .method_call::<(Vec<String>,), _, _, _>(BUS_NAME, "ListActivatableNames", ())
        .map(|(names,)| names.iter().any(|known| known == name))
        .unwrap_or(false)
}

/// Whether `name` answers or can be started.
pub fn name_available(connection: &Connection, name: &str) -> bool {
    name_has_owner(connection, name) || name_is_activatable(connection, name)
}

/// The names on the bus that start with `prefix`, sorted.
pub fn names_with_prefix(connection: &Connection, prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = bus_proxy(connection)
        .method_call::<(Vec<String>,), _, _, _>(BUS_NAME, "ListNames", ())
        .map(|(names,)| names)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.starts_with(prefix))
        .collect();
    names.sort();
    names
}
