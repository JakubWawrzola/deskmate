//! The seat on Linux: the same contract as on Windows (seat.rs), for the
//! daemon and the desktop apps of the users of one computer.
//!
//! The named objects are Unix sockets in the abstract namespace. Like the
//! `Global\` objects on Windows, every user of the computer sees them, no
//! file is left behind, and the kernel removes them when their process ends,
//! however it ends. Whoever binds the seat's name holds the connection; a
//! second name announces that somebody is waiting for it.
//!
//! The process that should have the connection is the desktop app of the
//! user at the screen. A holder steps back for a waiting one when it is not
//! that: the daemon always, a desktop app once its user has switched away.
//!
//! The names are visible to every process in the network namespace, so a
//! program in a sandbox with its own namespace does not take part.

use crate::link::ClientRole;
use std::io::ErrorKind;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Whether `machine_turn` has said that the seat cannot be checked.
static MACHINE_SEAT_WARNED: AtomicBool = AtomicBool::new(false);

/// A bound socket and the node it was bound for.
struct Held {
    node: String,
    _socket: UnixDatagram,
}

/// Whether this process is the one a person is using the computer through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    /// A desktop app: decided by the session it runs in.
    Session,
    /// The daemon never is. The tests use both values.
    Fixed(bool),
}

/// One participant: what the statics of the Windows implementation hold, as a
/// value, so that a test can have several in one process.
struct Holder {
    presence: Presence,
    seat: Option<Held>,
    waiting: Option<Held>,
}

/// This process.
static HOLDER: Mutex<Holder> = Mutex::new(Holder::new(Presence::Session));

fn holder() -> std::sync::MutexGuard<'static, Holder> {
    HOLDER.lock().unwrap_or_else(|e| e.into_inner())
}

/// Tells the seat what this process is. A desktop app is the default and
/// needs no call. The daemon calls this once at startup: it then keeps the
/// connection only while no desktop app is waiting for it, and never
/// announces that it waits itself.
pub fn set_role(role: ClientRole) {
    holder().presence = match role {
        ClientRole::App => Presence::Session,
        ClientRole::Service | ClientRole::Daemon => Presence::Fixed(false),
    };
}

pub fn turn(node: &str) -> bool {
    holder().turn(node)
}

pub fn set_waiting(node: &str, waiting: bool) {
    holder().set_waiting(node, waiting);
}

/// `seat::machine_turn` for the daemon: like `turn` for a holder that is never
/// in use, except that a false while holding the seat keeps it. The daemon
/// closes its connection first and then calls `release`, so a desktop app
/// connects only after the daemon has left.
pub fn machine_turn(node: &str) -> bool {
    holder().machine_turn(node)
}

pub fn release(node: &str) {
    holder().release(node);
}

fn seat_name(node: &str) -> String {
    format!("deskmate.seat.{node}")
}

fn waiting_name(node: &str) -> String {
    format!("deskmate.seat.{node}.waiting")
}

fn bind(name: &str) -> std::io::Result<UnixDatagram> {
    UnixDatagram::bind_addr(&SocketAddr::from_abstract_name(name)?)
}

/// Whether some process has bound `name`. Connecting a datagram socket
/// reaches nobody and sends nothing; it only succeeds when the name exists.
fn is_bound(name: &str) -> bool {
    SocketAddr::from_abstract_name(name)
        .and_then(|address| UnixDatagram::unbound()?.connect_addr(&address))
        .is_ok()
}

/// Where the kernel names the virtual terminal in the foreground.
const ACTIVE_TERMINAL: &str = "class/tty/tty0/active";

/// Whether the virtual terminal `number` is the one on the screen. Switching
/// to another user's session switches the terminal; locking the screen does
/// not. None when the kernel does not tell.
fn terminal_in_foreground(sys: &Path, number: &str) -> Option<bool> {
    let active = std::fs::read_to_string(sys.join(ACTIVE_TERMINAL)).ok()?;
    let active = active.trim().strip_prefix("tty")?;
    Some(active == number.trim())
}

/// Whether the session this process runs in is the one in use. A session
/// without a virtual terminal (a remote desktop, a second seat) cannot be
/// asked this way and counts as active, like an unknown state on Windows.
fn session_active() -> bool {
    std::env::var("XDG_VTNR")
        .ok()
        .and_then(|number| terminal_in_foreground(Path::new(crate::linux::SYS), &number))
        .unwrap_or(true)
}

impl Holder {
    const fn new(presence: Presence) -> Self {
        Self {
            presence,
            seat: None,
            waiting: None,
        }
    }

    fn active(&self) -> bool {
        match self.presence {
            Presence::Session => session_active(),
            Presence::Fixed(active) => active,
        }
    }

    fn set_waiting(&mut self, node: &str, waiting: bool) {
        if let Some(held) = &self.waiting {
            if waiting && held.node == node {
                return;
            }
            self.waiting = None;
        }
        if waiting {
            // Fails when another process already waits; the announcement
            // exists then anyway.
            if let Ok(socket) = bind(&waiting_name(node)) {
                self.waiting = Some(Held {
                    node: node.to_string(),
                    _socket: socket,
                });
            }
        }
    }

    fn turn(&mut self, node: &str) -> bool {
        let active = self.active();

        if let Some(held) = &self.seat {
            if held.node == node {
                if !active && is_bound(&waiting_name(node)) {
                    log::info!(
                        "seat: the desktop app of the user at the screen is waiting, handing over"
                    );
                    self.seat = None;
                    return false;
                }
                return true;
            }
            // Node renamed: the old seat is no longer ours to keep.
            self.seat = None;
        }

        // Our own announcement must not count when we check for others.
        self.set_waiting(node, false);
        if !active && is_bound(&waiting_name(node)) {
            return false;
        }
        match bind(&seat_name(node)) {
            Ok(socket) => {
                self.seat = Some(Held {
                    node: node.to_string(),
                    _socket: socket,
                });
                return true;
            }
            // Another process holds it.
            Err(error) if error.kind() == ErrorKind::AddrInUse => {}
            Err(error) => {
                // The seat cannot be checked at all; do not stop the program
                // from connecting because of that.
                log::warn!("seat: cannot bind the seat socket ({error}); connecting anyway");
                return true;
            }
        }
        if active {
            self.set_waiting(node, true);
        }
        false
    }

    fn machine_turn(&mut self, node: &str) -> bool {
        if let Some(held) = &self.seat {
            if held.node == node {
                return !is_bound(&waiting_name(node));
            }
            self.seat = None;
        }
        if is_bound(&waiting_name(node)) {
            return false;
        }
        match bind(&seat_name(node)) {
            Ok(socket) => {
                self.seat = Some(Held {
                    node: node.to_string(),
                    _socket: socket,
                });
                true
            }
            Err(error) if error.kind() == ErrorKind::AddrInUse => false,
            Err(error) => {
                // Asked every few seconds; said once.
                if !MACHINE_SEAT_WARNED.swap(true, Ordering::Relaxed) {
                    log::warn!("seat: cannot bind the seat socket ({error}); connecting anyway");
                }
                true
            }
        }
    }

    fn release(&mut self, node: &str) {
        if self.seat.as_ref().is_some_and(|held| held.node == node) {
            self.seat = None;
        }
        if self.waiting.as_ref().is_some_and(|held| held.node == node) {
            self.waiting = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A node of its own for every test: the names are shared by the whole
    /// computer, and the test binary may run several times at once.
    fn node() -> String {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        format!(
            "test_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn app() -> Holder {
        Holder::new(Presence::Fixed(true))
    }

    fn daemon() -> Holder {
        Holder::new(Presence::Fixed(false))
    }

    #[test]
    fn a_single_process_holds_the_seat_and_never_notices() {
        let node = node();
        for mut only in [app(), daemon()] {
            assert!(only.turn(&node));
            assert!(only.turn(&node));
            assert!(only.waiting.is_none());
        }
    }

    #[test]
    fn the_daemon_yields_to_a_waiting_desktop_app() {
        let node = node();
        let mut daemon = daemon();
        let mut app = app();

        assert!(daemon.turn(&node));
        assert!(!app.turn(&node), "the seat is taken");
        assert!(app.waiting.is_some(), "the app announces that it waits");

        assert!(!daemon.turn(&node), "the daemon hands over");
        assert!(daemon.seat.is_none(), "and has already let go");
        assert!(app.turn(&node));
        assert!(app.waiting.is_none(), "nobody waits any more");

        assert!(!daemon.turn(&node));
        assert!(daemon.waiting.is_none(), "the daemon waits without a word");
        assert!(app.turn(&node), "so the app keeps the seat");

        drop(app);
        assert!(daemon.turn(&node), "the app is gone, the daemon is back");
    }

    #[test]
    fn an_app_in_use_keeps_the_seat_until_its_user_switches_away() {
        let node = node();
        let mut first = app();
        let mut second = app();

        assert!(first.turn(&node));
        assert!(!second.turn(&node));
        assert!(first.turn(&node), "both are in use: the holder stays");
        assert!(!second.turn(&node));

        // The first user switches to the second one's session.
        first.presence = Presence::Fixed(false);
        assert!(!first.turn(&node));
        assert!(second.turn(&node));
        assert!(!first.turn(&node));
        assert!(first.waiting.is_none());
    }

    #[test]
    fn two_daemons_never_take_the_seat_from_each_other() {
        let node = node();
        let mut first = daemon();
        let mut second = daemon();
        assert!(first.turn(&node));
        assert!(!second.turn(&node));
        assert!(first.turn(&node), "nobody announced anything");
    }

    #[test]
    fn the_daemon_keeps_the_seat_until_it_releases_it() {
        let node = node();
        let mut daemon = daemon();
        let mut app = app();

        assert!(daemon.machine_turn(&node));
        assert!(daemon.machine_turn(&node), "nobody waits");
        assert!(!app.turn(&node), "the seat is taken");
        assert!(app.waiting.is_some());

        assert!(!daemon.machine_turn(&node), "an app waits");
        assert!(daemon.seat.is_some(), "but the daemon still holds the seat");
        assert!(!app.turn(&node), "so the app cannot connect yet");

        daemon.release(&node);
        assert!(daemon.seat.is_none());
        assert!(app.turn(&node), "the daemon has left");
        assert!(!daemon.machine_turn(&node), "the app holds the seat");
        assert!(
            daemon.waiting.is_none(),
            "the daemon never announces that it waits"
        );

        drop(app);
        assert!(
            daemon.machine_turn(&node),
            "the app is gone, the daemon is back"
        );
    }

    #[test]
    fn the_daemon_does_not_take_a_free_seat_an_app_waits_for() {
        let node = node();
        let mut app = app();
        app.set_waiting(&node, true);
        let mut daemon = daemon();
        assert!(!daemon.machine_turn(&node));
        assert!(daemon.seat.is_none());
    }

    #[test]
    fn release_of_another_node_keeps_the_seat() {
        let (held, other) = (node(), node());
        let mut daemon = daemon();
        assert!(daemon.machine_turn(&held));
        daemon.release(&other);
        assert!(daemon.seat.is_some());
        assert!(!app().turn(&held));
    }

    #[test]
    fn a_waiting_process_that_stands_down_is_no_longer_waited_for() {
        let node = node();
        let mut daemon = daemon();
        let mut app = app();
        assert!(daemon.turn(&node));
        assert!(!app.turn(&node));
        app.set_waiting(&node, false);
        assert!(daemon.turn(&node));
    }

    #[test]
    fn a_renamed_node_releases_the_old_seat() {
        let (old, new) = (node(), node());
        let mut holder = app();
        let mut other = app();
        assert!(holder.turn(&old));
        assert!(holder.turn(&new));
        assert!(other.turn(&old), "the old seat is free again");
        assert!(!other.turn(&new));
    }

    #[test]
    fn nodes_do_not_share_a_seat() {
        let mut first = app();
        let mut second = app();
        assert!(first.turn(&node()));
        assert!(second.turn(&node()));
    }

    #[test]
    fn a_name_too_long_for_a_socket_does_not_block_the_connection() {
        let mut holder = daemon();
        assert!(holder.turn(&"x".repeat(200)));
        assert!(holder.seat.is_none());
    }

    /// The only test that uses the holder of the process itself, through the
    /// functions the connection loop calls.
    #[tokio::test]
    async fn this_process_waits_for_its_turn_and_takes_it() {
        let node = node();
        holder().presence = Presence::Fixed(true);
        let mut other = app();
        assert!(other.turn(&node));

        let (stop, mut stopped) = tokio::sync::watch::channel(false);
        let mut announced = Vec::new();
        let waited = crate::seat::wait_for_turn(&node, &mut stopped, |status| {
            announced.push(status.to_string())
        });
        let stopping = async {
            // Lets the waiting side run up to its first pause.
            tokio::task::yield_now().await;
            assert!(is_bound(&waiting_name(&node)), "it announced that it waits");
            stop.send(true).unwrap();
        };
        let (got_the_seat, ()) = tokio::join!(waited, stopping);
        assert!(!got_the_seat, "stopped while waiting");
        assert_eq!(announced, [crate::seat::WAITING_STATUS]);
        assert!(!is_bound(&waiting_name(&node)), "it stood down");

        drop(other);
        assert!(turn(&node));
        assert!(turn(&node));

        set_role(ClientRole::Daemon);
        let mut app = app();
        assert!(!app.turn(&node));
        assert!(!turn(&node), "as the daemon it hands over");
        assert!(app.turn(&node));
    }

    #[test]
    fn the_session_is_in_use_while_its_terminal_is_on_the_screen() {
        let tree = Tree::new();
        assert_eq!(terminal_in_foreground(tree.path(), "2"), None);
        tree.file("class/tty/tty0/active", "tty2\n");
        assert_eq!(terminal_in_foreground(tree.path(), "2"), Some(true));
        assert_eq!(terminal_in_foreground(tree.path(), "3"), Some(false));
        assert_eq!(terminal_in_foreground(tree.path(), "22"), Some(false));
        tree.file("class/tty/tty0/active", "\n");
        assert_eq!(terminal_in_foreground(tree.path(), "2"), None);
    }
}
