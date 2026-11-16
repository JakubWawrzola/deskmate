//! The seat between the desktop app and the daemon, in two processes: this
//! test is the app (the role a desktop app has without a call), and it
//! starts the test binary again as the daemon, which calls
//! `seat::linux::set_role(ClientRole::Daemon)` as the daemon's host does.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use deskmate_core::link::ClientRole;

/// Set for the daemon process: the node both processes use.
const CHILD_NODE: &str = "DESKMATE_SEAT_TEST_NODE";
/// The answers of the daemon process follow this; the rest is the test
/// harness talking.
const ANSWER: &str = "SEAT ";

struct Daemon {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Daemon {
    fn start(node: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux::seat_tests::daemon_process",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_NODE, node)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }

    fn ask(&mut self, request: &str) -> String {
        writeln!(self.input, "{request}").unwrap();
        self.input.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "the daemon process ended"
            );
            // The harness may have written the test's name on the same line.
            if let Some((_, answer)) = line.trim().split_once(ANSWER) {
                return answer.to_string();
            }
        }
    }

    fn stop(mut self) {
        let _ = writeln!(self.input, "exit");
        drop(self.input);
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn the_desktop_app_takes_the_seat_from_the_daemon_and_gives_it_back() {
    let node = format!("test_app_daemon_{}", std::process::id());
    let mut daemon = Daemon::start(&node);

    assert_eq!(
        daemon.ask("machine_turn"),
        "true",
        "the daemon takes a free seat"
    );
    assert!(
        !crate::seat::turn(&node),
        "the app finds it taken and waits"
    );
    assert_eq!(
        daemon.ask("machine_turn"),
        "false",
        "the daemon sees the app waiting"
    );
    assert_eq!(daemon.ask("release"), "released", "it closes and lets go");
    assert!(crate::seat::turn(&node), "the app takes the seat");
    assert_eq!(
        daemon.ask("machine_turn"),
        "false",
        "the daemon waits without taking it"
    );
    assert!(crate::seat::turn(&node), "the app keeps it");

    crate::seat::release(&node);
    assert_eq!(
        daemon.ask("machine_turn"),
        "true",
        "the app is gone, the daemon is back"
    );
    daemon.stop();
}

/// The daemon's side; runs only when the test above starts it.
#[test]
#[ignore = "the daemon process of the test above"]
fn daemon_process() {
    let Ok(node) = std::env::var(CHILD_NODE) else {
        return;
    };
    deskmate_core::seat::linux::set_role(ClientRole::Daemon);
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().trim() {
            "machine_turn" => println!("{ANSWER}{}", crate::seat::machine_turn(&node)),
            "release" => {
                crate::seat::release(&node);
                println!("{ANSWER}released");
            }
            _ => break,
        }
    }
}
