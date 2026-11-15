//! The `deskmate-service` program: the Windows service and the commands
//! that install it, remove it and ask for its status. The command line is
//! described in `deskmate_service::cli`.

use std::process::ExitCode;

/// Exit code for a wrong command line.
const EXIT_USAGE: u8 = 2;

#[cfg(windows)]
mod windows_main {
    use deskmate_service::cli::{Command, RunOptions, USAGE};
    use deskmate_service::secrets::{CredentialVault, KeyStore};
    use deskmate_service::win::install::{install, uninstall, InstallOptions, APP_PROGRAM};
    use deskmate_service::win::pipes::{read_status, PipeNames};
    use deskmate_service::win::security::is_elevated;
    use deskmate_service::win::{data_dir, service, Launch};
    use std::process::ExitCode;
    use std::sync::Arc;

    /// Exit code of `install` and `uninstall` without an elevated terminal:
    /// the Windows error "access denied", so a caller can tell it apart.
    const EXIT_NOT_ELEVATED: u8 = 5;

    /// What a run works with: the real folder, vault, pipes and power
    /// commands.
    #[cfg(not(feature = "test-hooks"))]
    fn launch(_options: &RunOptions, echo_log: bool) -> Result<Launch, String> {
        Ok(Launch {
            data_dir: data_dir()?,
            check_folder: true,
            keys: Arc::new(CredentialVault) as Arc<dyn KeyStore>,
            power: Arc::new(deskmate_service::power::SystemPower),
            pipes: PipeNames::standard(),
            echo_log,
        })
    }

    /// What a test run works with. Power commands are a dry run, always: a
    /// build with test hooks has no other executor.
    #[cfg(feature = "test-hooks")]
    fn launch(options: &RunOptions, echo_log: bool) -> Result<Launch, String> {
        if !options.dry_run_power {
            return Err(
                "this is a test build: it records power commands and never runs them. Pass --dry-run-power to say so"
                    .into(),
            );
        }
        let keys: Arc<dyn KeyStore> = match &options.key_file {
            Some(path) => Arc::new(deskmate_service::secrets::KeyFile::new(path.clone())),
            None => Arc::new(CredentialVault),
        };
        Ok(Launch {
            data_dir: match &options.data_dir {
                Some(dir) => dir.clone(),
                None => data_dir()?,
            },
            check_folder: options.data_dir.is_none(),
            keys,
            power: Arc::new(deskmate_service::power::DryRunPower),
            pipes: match &options.pipe_suffix {
                Some(suffix) => PipeNames::with_suffix(suffix),
                None => PipeNames::standard(),
            },
            echo_log,
        })
    }

    /// Runs the service in this terminal. Events come from stdin, one word
    /// per line: stop, shutdown, suspend, resume, session, reload. The end
    /// of stdin is a stop.
    #[cfg(feature = "test-hooks")]
    fn console(options: &RunOptions) -> Result<(), String> {
        use deskmate_service::runner::{event_channel, Ack, Event};
        use std::io::Write;
        use std::time::Duration;

        let launch = launch(options, true)?;
        let (events, incoming) = event_channel();
        let from_stdin = events.clone();
        std::thread::spawn(move || {
            for line in std::io::stdin().lines() {
                let Ok(line) = line else { break };
                let event = match line.trim() {
                    "stop" => Event::Stop,
                    "shutdown" => Event::Shutdown,
                    "suspend" => {
                        // Like the service's control handler: wait until the
                        // connection is closed.
                        let (ack, waiter) = Ack::pair();
                        if from_stdin.send(Event::Suspend(ack)).is_ok() {
                            waiter.wait(Duration::from_millis(1500));
                        }
                        continue;
                    }
                    "resume" => Event::Resume,
                    "session" => Event::SessionChange,
                    "reload" => Event::Reload,
                    _ => continue,
                };
                if from_stdin.send(event).is_err() {
                    return;
                }
            }
            let _ = from_stdin.send(Event::Stop);
        });
        deskmate_service::win::host(launch, events, incoming, || {
            // Tells whoever drives the run that the pipes are open.
            println!("ready");
            let _ = std::io::stdout().flush();
        })
        .map_err(|error| error.message().to_string())
    }

    fn needs_elevation(command: &str) -> ExitCode {
        eprintln!(
            "deskmate-service: `{command}` changes the system and must be run from an elevated terminal (Run as administrator)"
        );
        ExitCode::from(EXIT_NOT_ELEVATED)
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

    pub fn dispatch(command: Command) -> ExitCode {
        let mut say = |line: &str| println!("{line}");
        match command {
            Command::Help => {
                println!("{USAGE}");
                ExitCode::SUCCESS
            }
            Command::Version => {
                println!("deskmate-service {}", env!("CARGO_PKG_VERSION"));
                ExitCode::SUCCESS
            }
            Command::Run(options) => finish(launch(&options, false).and_then(service::run)),
            #[cfg(feature = "test-hooks")]
            Command::Console(options) => finish(console(&options)),
            #[cfg(not(feature = "test-hooks"))]
            Command::Install { provision_pipe } => {
                if !is_elevated() {
                    return needs_elevation("install");
                }
                let options = InstallOptions {
                    provision_pipe,
                    provision_program: APP_PROGRAM.into(),
                    run_arguments: Vec::new(),
                };
                finish(install(&options, &mut say))
            }
            #[cfg(feature = "test-hooks")]
            Command::Install {
                provision_pipe,
                dry_run_power,
                provision_program,
            } => {
                if !is_elevated() {
                    return needs_elevation("install");
                }
                if !dry_run_power {
                    eprintln!(
                        "deskmate-service: this is a test build; install it with --dry-run-power"
                    );
                    return ExitCode::from(super::EXIT_USAGE);
                }
                let options = InstallOptions {
                    provision_pipe,
                    provision_program: provision_program.unwrap_or_else(|| APP_PROGRAM.into()),
                    run_arguments: vec!["--dry-run-power".into()],
                };
                finish(install(&options, &mut say))
            }
            Command::Uninstall => {
                if !is_elevated() {
                    return needs_elevation("uninstall");
                }
                finish(uninstall(&mut say))
            }
            #[cfg(not(feature = "test-hooks"))]
            Command::Status {} => {
                finish(read_status(&PipeNames::standard()).map(|line| say(&line)))
            }
            #[cfg(feature = "test-hooks")]
            Command::Status { pipe_suffix } => {
                let names = match pipe_suffix {
                    Some(suffix) => PipeNames::with_suffix(&suffix),
                    None => PipeNames::standard(),
                };
                finish(read_status(&names).map(|line| say(&line)))
            }
        }
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = match deskmate_service::cli::parse(&arguments) {
        Ok(command) => command,
        Err(error) => {
            eprintln!(
                "deskmate-service: {error}\n\n{}",
                deskmate_service::cli::USAGE
            );
            return ExitCode::from(EXIT_USAGE);
        }
    };
    #[cfg(windows)]
    {
        windows_main::dispatch(command)
    }
    #[cfg(not(windows))]
    {
        let _ = command;
        eprintln!("deskmate-service: this program runs on Windows only for now");
        ExitCode::FAILURE
    }
}
