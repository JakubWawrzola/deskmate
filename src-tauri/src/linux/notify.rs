//! Notifications through the desktop's notification service
//! (`org.freedesktop.Notifications` on the session bus), with buttons where
//! the service offers them.
//!
//! One thread owns the connection. It shows the notifications and hears
//! the clicks on their buttons: several services send `ActionInvoked` only to
//! the connection that showed the notification. A button carries a random
//! key, not the action's name; a click counts only with a key this process
//! put on a notification it showed, once, as the tokens of the Windows
//! toasts do (notify.rs).

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::message::MatchRule;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::bus;
use crate::notify::{NotifyPayload, OwnButton};
use crate::state::AppState;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const APP_NAME: &str = "Deskmate";
/// The icon and the `.desktop` file that the Linux packages install.
const APP_ICON: &str = "deskmate";
const DESKTOP_ENTRY: &str = "Deskmate";
/// The service decides how long a notification stays.
const SERVER_DEFAULT_TIMEOUT: i32 = -1;
/// How long a caller waits for the notification thread.
const SHOW_TIMEOUT: Duration = Duration::from_secs(3);
/// How often the thread looks for clicks while nothing is to be shown.
const POLL_EVERY: Duration = Duration::from_millis(250);
/// After the session bus went away.
const RECONNECT_AFTER: Duration = Duration::from_secs(10);
/// Notifications whose buttons are remembered at most.
const MAX_PENDING: usize = 256;
/// The folder for downloaded notification images, inside the user's
/// runtime folder.
const IMAGE_FOLDER: &str = "deskmate";

/// Where a click on a button goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// The action name of a notification from Home Assistant, sent back to it.
    HomeAssistant(String),
    /// A `deskmate:` link of a notification from Deskmate itself.
    Own(String),
}

#[derive(Debug, Clone)]
struct Button {
    key: String,
    label: String,
    target: Target,
}

struct Job {
    summary: String,
    body: String,
    image: Option<PathBuf>,
    buttons: Vec<Button>,
    done: mpsc::Sender<Result<(), String>>,
}

/// The buttons of the notifications on screen, by notification id.
type Pending = Arc<Mutex<HashMap<u32, Vec<Button>>>>;

static JOBS: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

/// Starts the notification thread. Called once from setup.
pub fn start(app: AppHandle) {
    let (sender, receiver) = mpsc::channel();
    if JOBS.set(Mutex::new(sender)).is_err() {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("notifications".into())
        .spawn(move || run(&app, &receiver))
    {
        log::warn!("notifications: cannot start the thread: {error}");
    }
}

/// Shows a notification. `image` is a local file; `own_buttons` are buttons
/// of Deskmate's own notifications, handled inside the app.
pub fn show(
    payload: &NotifyPayload,
    image: Option<&Path>,
    own_buttons: &[OwnButton],
) -> Result<(), String> {
    let mut buttons: Vec<Button> = payload
        .actions
        .iter()
        .filter(|action| !action.title.is_empty() && !action.action.is_empty())
        .map(|action| Button {
            key: random_key(),
            label: action.title.clone(),
            target: Target::HomeAssistant(action.action.clone()),
        })
        .collect();
    buttons.extend(own_buttons.iter().map(|button| Button {
        key: random_key(),
        label: button.title.clone(),
        target: Target::Own(button.link.clone()),
    }));
    let (done, outcome) = mpsc::channel();
    let job = Job {
        summary: payload.title.clone(),
        body: payload.message.clone(),
        image: image.map(Path::to_path_buf),
        buttons,
        done,
    };
    JOBS.get()
        .ok_or("notifications are not ready yet")?
        .lock()
        .map_err(|_| "notifications are unavailable")?
        .send(job)
        .map_err(|_| "the notification thread has stopped")?;
    outcome
        .recv_timeout(SHOW_TIMEOUT)
        .map_err(|_| "the notification service did not answer".to_string())?
}

/// Whether a notification service answers or can be started.
pub fn service_available(connection: &Connection) -> bool {
    bus::name_available(connection, SERVICE)
}

/// What the service supports (`actions`, `body-markup`, ...).
pub fn service_capabilities(connection: &Connection) -> Vec<String> {
    connection
        .with_proxy(SERVICE, PATH, bus::CALL_TIMEOUT)
        .method_call::<(Vec<String>,), _, _, _>(SERVICE, "GetCapabilities", ())
        .map(|(capabilities,)| capabilities)
        .unwrap_or_default()
}

/// The folder for downloaded notification images: in the user's runtime
/// folder, which only the user can enter, instead of the shared /tmp.
pub fn image_dir() -> PathBuf {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        return std::env::temp_dir();
    };
    let dir = runtime.join(IMAGE_FOLDER);
    if let Err(error) = create_private_dir(&dir) {
        log::warn!("notifications: cannot create the image folder: {error}");
    }
    dir
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => Err(error),
        _ => Ok(()),
    }
}

fn run(app: &AppHandle, jobs: &mpsc::Receiver<Job>) {
    loop {
        match bus::session() {
            Ok(connection) => match serve(app, &connection, jobs) {
                Served::Stopped => return,
                Served::Lost(error) => log::warn!("notifications: {error}"),
            },
            Err(error) => log::warn!("notifications: {error}"),
        }
        // Without a bus, every notification fails at once instead of waiting.
        let until = std::time::Instant::now() + RECONNECT_AFTER;
        while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
            match jobs.recv_timeout(left) {
                Ok(job) => {
                    let _ = job.done.send(Err("no session bus".into()));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

enum Served {
    Stopped,
    Lost(String),
}

fn serve(app: &AppHandle, connection: &Connection, jobs: &mpsc::Receiver<Job>) -> Served {
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    if let Err(error) = listen(app, connection, &pending) {
        return Served::Lost(error);
    }
    loop {
        match jobs.recv_timeout(POLL_EVERY) {
            Ok(job) => {
                let result = notify(connection, &pending, &job);
                let _ = job.done.send(result);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Served::Stopped,
        }
        if let Err(error) = connection.process(Duration::ZERO) {
            return Served::Lost(error.to_string());
        }
    }
}

/// Hears clicks and closed notifications, from the service only.
fn listen(app: &AppHandle, connection: &Connection, pending: &Pending) -> Result<(), String> {
    let clicks = Arc::clone(pending);
    let app = app.clone();
    connection
        .add_match(
            MatchRule::new_signal(SERVICE, "ActionInvoked").with_sender(SERVICE),
            move |(id, key): (u32, String), _: &Connection, _: &dbus::Message| {
                if let Some(target) = redeem(&clicks, id, &key) {
                    dispatch(&app, target);
                }
                true
            },
        )
        .map_err(|e| format!("cannot listen for clicks: {e}"))?;
    let closed = Arc::clone(pending);
    connection
        .add_match(
            MatchRule::new_signal(SERVICE, "NotificationClosed").with_sender(SERVICE),
            move |(id, _reason): (u32, u32), _: &Connection, _: &dbus::Message| {
                if let Ok(mut map) = closed.lock() {
                    map.remove(&id);
                }
                true
            },
        )
        .map_err(|e| format!("cannot listen for closed notifications: {e}"))?;
    Ok(())
}

/// The target of a click, once: the notification's buttons are forgotten
/// with the first click on any of them.
fn redeem(pending: &Pending, id: u32, key: &str) -> Option<Target> {
    let mut map = pending.lock().ok()?;
    let buttons = map.get(&id)?;
    let target = buttons
        .iter()
        .find(|button| button.key == key)
        .map(|button| button.target.clone());
    if target.is_some() {
        map.remove(&id);
    } else {
        crate::security::audit("toast_action", "blocked_unknown_token");
    }
    target
}

fn dispatch(app: &AppHandle, target: Target) {
    match target {
        Target::HomeAssistant(action) => {
            if app.state::<AppState>().action_tx.send(action).is_err() {
                log::warn!("notifications: the action channel is closed");
            }
        }
        Target::Own(link) => {
            if !crate::updates::handle_link(app, &link) {
                log::warn!("notifications: a button of Deskmate's own was not understood");
            }
        }
    }
}

fn notify(connection: &Connection, pending: &Pending, job: &Job) -> Result<(), String> {
    let capabilities = service_capabilities(connection);
    let has = |name: &str| capabilities.iter().any(|capability| capability == name);
    let body = if has("body-markup") {
        escape_markup(&job.body)
    } else {
        job.body.clone()
    };
    let buttons: &[Button] = if has("actions") { &job.buttons } else { &[] };
    let actions: Vec<String> = buttons
        .iter()
        .flat_map(|button| [button.key.clone(), button.label.clone()])
        .collect();
    let mut hints = PropMap::new();
    hints.insert("desktop-entry".into(), text_variant(DESKTOP_ENTRY));
    if let Some(uri) = job
        .image
        .as_deref()
        .and_then(|path| url::Url::from_file_path(path).ok())
    {
        hints.insert("image-path".into(), text_variant(uri.as_str()));
    }
    let (id,): (u32,) = connection
        .with_proxy(SERVICE, PATH, bus::CALL_TIMEOUT)
        .method_call(
            SERVICE,
            "Notify",
            (
                APP_NAME,
                0u32,
                APP_ICON,
                job.summary.as_str(),
                body.as_str(),
                actions,
                hints,
                SERVER_DEFAULT_TIMEOUT,
            ),
        )
        .map_err(|e| format!("notification: {e}"))?;
    if !buttons.is_empty() {
        remember(pending, id, buttons.to_vec());
    }
    Ok(())
}

fn remember(pending: &Pending, id: u32, buttons: Vec<Button>) {
    let Ok(mut map) = pending.lock() else {
        return;
    };
    if map.len() >= MAX_PENDING {
        // Ids grow, so the smallest is the oldest notification.
        if let Some(oldest) = map.keys().min().copied() {
            map.remove(&oldest);
        }
    }
    map.insert(id, buttons);
}

fn text_variant(value: &str) -> Variant<Box<dyn RefArg>> {
    Variant(Box::new(value.to_string()))
}

/// The service reads the body as markup when it says `body-markup`: text
/// from Home Assistant must stay text there, never a link or an image.
fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn random_key() -> String {
    use rand::Rng;
    let mut raw = [0u8; 16];
    // Panics only if the system's random source fails, as rand 0.8's OsRng did.
    rand::rand_core::UnwrapErr(rand::rngs::SysRng).fill_bytes(&mut raw);
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(key: &str, target: Target) -> Button {
        Button {
            key: key.into(),
            label: "Label".into(),
            target,
        }
    }

    #[test]
    fn a_click_counts_once_and_only_with_a_key_that_was_issued() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        remember(
            &pending,
            7,
            vec![
                button("k1", Target::HomeAssistant("ok".into())),
                button("k2", Target::Own("deskmate:update?later".into())),
            ],
        );
        assert_eq!(redeem(&pending, 7, "forged"), None);
        assert_eq!(redeem(&pending, 8, "k1"), None, "another notification");
        assert_eq!(
            redeem(&pending, 7, "k1"),
            Some(Target::HomeAssistant("ok".into()))
        );
        assert_eq!(redeem(&pending, 7, "k2"), None, "the notification is done");
    }

    #[test]
    fn the_oldest_notification_makes_room() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        for id in 0..MAX_PENDING as u32 + 1 {
            remember(
                &pending,
                id,
                vec![button("k", Target::HomeAssistant("a".into()))],
            );
        }
        let map = pending.lock().unwrap();
        assert_eq!(map.len(), MAX_PENDING);
        assert!(!map.contains_key(&0));
        assert!(map.contains_key(&(MAX_PENDING as u32)));
    }

    #[test]
    fn markup_from_home_assistant_stays_text() {
        assert_eq!(
            escape_markup("<a href=\"x\">1 & 2</a>"),
            "&lt;a href=\"x\"&gt;1 &amp; 2&lt;/a&gt;"
        );
    }

    #[test]
    fn button_keys_are_random_and_long() {
        let (first, second) = (random_key(), random_key());
        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
    }
}
