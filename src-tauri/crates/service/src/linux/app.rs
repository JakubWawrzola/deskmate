//! What `deskmate-service` does on Linux, command by command
//! (`linux::cli`).

use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Command as Process, ExitCode, Stdio};
use std::sync::Arc;
use zeroize::Zeroizing;

use super::cli::{parse, Command, Options, USAGE};
use super::files;
use super::pair::{pair, unpair, Target};
use super::{host, journal, status_file, Launch, Paths, DAEMON_USER, UNIT};
use crate::runner::event_channel;

const EXIT_USAGE: u8 = 2;
/// Exit code of `pair` and `unpair` without root, as `install` on Windows
/// uses for a terminal that is not elevated.
const EXIT_NOT_ROOT: u8 = 5;
/// A pasted pairing code is far shorter than this.
const MAX_CODE_BYTES: u64 = 8192;
/// Exists while systemd runs the system.
const SYSTEMD_RUNNING: &str = "/run/systemd/system";
const SYSTEMCTL: &str = "systemctl";

/// Runs the command line `arguments` (without the program's name).
pub fn main(arguments: &[String]) -> ExitCode {
    let command = match parse(arguments) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("deskmate-service: {error}\n\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match command {
        Command::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("deskmate-service {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Run(options) => finish(run(&options)),
        #[cfg(feature = "test-hooks")]
        Command::Console(options) => finish(console(&options)),
        Command::Pair(options) => pairing_command(&options, true),
        Command::Unpair(options) => pairing_command(&options, false),
        Command::Status(options) => {
            finish(status_file::read(&paths(&options).status_file).map(|line| println!("{line}")))
        }
    }
}

fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("deskmate-service: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The real folders and status file.
#[cfg(not(feature = "test-hooks"))]
fn paths(_options: &Options) -> Paths {
    Paths::system()
}

/// The real ones, or the ones a test run points at.
#[cfg(feature = "test-hooks")]
fn paths(options: &Options) -> Paths {
    let system = Paths::system();
    Paths {
        config_dir: options.config_dir.clone().unwrap_or(system.config_dir),
        state_dir: options.data_dir.clone().unwrap_or(system.state_dir),
        status_file: options.status_file.clone().unwrap_or(system.status_file),
    }
}

/// Whether a test run brings folders of its own.
#[cfg(not(feature = "test-hooks"))]
fn own_folders(_options: &Options) -> bool {
    false
}

#[cfg(feature = "test-hooks")]
fn own_folders(options: &Options) -> bool {
    options.config_dir.is_some() || options.data_dir.is_some()
}

/// What a run works with: the real folders and the real power commands.
#[cfg(not(feature = "test-hooks"))]
fn launch(options: &Options, ask_systemd: bool) -> Result<Launch, String> {
    Ok(Launch {
        paths: paths(options),
        power: Arc::new(crate::power::SystemPower),
        check_folders: true,
        ask_systemd,
    })
}

/// What a test run works with. Power commands are a dry run, always: a build
/// with test hooks has no other executor.
#[cfg(feature = "test-hooks")]
fn launch(options: &Options, ask_systemd: bool) -> Result<Launch, String> {
    if !options.dry_run_power {
        return Err(
            "this is a test build: it records power commands and never runs them. Pass --dry-run-power to say so"
                .into(),
        );
    }
    Ok(Launch {
        paths: paths(options),
        power: Arc::new(crate::power::DryRunPower),
        check_folders: !own_folders(options),
        ask_systemd,
    })
}

/// The daemon as systemd starts it.
fn run(options: &Options) -> Result<(), String> {
    let launch = launch(options, true)?;
    journal::init()?;
    let (events, incoming) = event_channel();
    host(launch, events, incoming, || {})
}

/// The daemon in this terminal. Signals work as under systemd, except that
/// SIGTERM is always a plain stop. Events also come from stdin, one word per
/// line: stop, shutdown, suspend, resume, reload. The end of stdin is a stop.
#[cfg(feature = "test-hooks")]
fn console(options: &Options) -> Result<(), String> {
    use crate::runner::{Ack, Event};

    let launch = launch(options, false)?;
    journal::init()?;
    let (events, incoming) = event_channel();
    let from_stdin = events.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            let event = match line.trim() {
                "stop" => Event::Stop,
                "shutdown" => Event::Shutdown,
                "suspend" => Event::Suspend(Ack::none()),
                "resume" => Event::Resume,
                "reload" => Event::Reload,
                _ => continue,
            };
            if from_stdin.send(event).is_err() {
                return;
            }
        }
        let _ = from_stdin.send(Event::Stop);
    });
    host(launch, events, incoming, || {
        // Tells whoever drives the run that the daemon is listening.
        println!("ready");
        let _ = std::io::stdout().flush();
    })
}

/// `pair` or `unpair`: root writes for the daemon's user. A test run in
/// folders of its own writes as whoever runs it.
fn pairing_command(options: &Options, pairing: bool) -> ExitCode {
    let paths = paths(options);
    let owner = if own_folders(options) {
        None
    } else {
        match files::effective_uid() {
            Ok(0) => {}
            Ok(_) => {
                let command = if pairing { "pair" } else { "unpair" };
                eprintln!(
                    "deskmate-service: `{command}` writes the daemon's settings and keys and must be run as root: sudo deskmate-service {command}"
                );
                return ExitCode::from(EXIT_NOT_ROOT);
            }
            Err(error) => return finish(Err(error)),
        }
        match files::daemon_account(DAEMON_USER) {
            Ok(account) => Some(account),
            Err(error) => return finish(Err(error)),
        }
    };
    let target = Target { paths, owner };
    let result = if pairing {
        pair_from_stdin(&target)
    } else {
        unpair(&target).map(|()| println!("The daemon's settings and keys are removed."))
    };
    if result.is_ok() && owner.is_some() {
        tell_the_daemon();
    }
    finish(result)
}

fn pair_from_stdin(target: &Target) -> Result<(), String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprint!("Paste the pairing code from the Deskmate integration in Home Assistant and press Enter: ");
        let _ = std::io::stderr().flush();
    }
    // Room for the whole line up front: a buffer that grows would leave
    // copies of the key behind in freed memory.
    let mut code = Zeroizing::new(String::with_capacity(MAX_CODE_BYTES as usize + 1));
    stdin
        .lock()
        .take(MAX_CODE_BYTES + 1)
        .read_line(&mut code)
        .map_err(|e| format!("cannot read the pairing code: {e}"))?;
    if code.len() as u64 > MAX_CODE_BYTES {
        return Err("that is too long to be a pairing code".into());
    }
    let config = pair(&code, target)?;
    println!(
        "Paired as {}. Settings: {}. Keys: {}.",
        config.node_id,
        target
            .paths
            .config_dir
            .join(crate::config::FILE_NAME)
            .display(),
        target.paths.state_dir.display()
    );
    Ok(())
}

/// Asks systemd to give the running daemon the new settings. A daemon that is
/// not running reads them when it starts.
fn tell_the_daemon() {
    if !Path::new(SYSTEMD_RUNNING).exists() {
        println!("Start the daemon to connect: systemctl start {UNIT}");
        return;
    }
    let told = Process::new(SYSTEMCTL)
        .args(["try-reload-or-restart", UNIT])
        .stdin(Stdio::null())
        .status();
    match told {
        Ok(status) if status.success() => println!(
            "A running daemon reads the new settings now; a stopped one when it starts (sudo systemctl enable --now {UNIT}). See: deskmate-service status"
        ),
        _ => println!("Tell the daemon yourself: sudo systemctl reload-or-restart {UNIT}"),
    }
}
