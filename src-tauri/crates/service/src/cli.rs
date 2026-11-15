//! The command line of `deskmate-service`.
//!
//! ```text
//! deskmate-service run
//! deskmate-service install [--provision-pipe <name>]
//! deskmate-service uninstall
//! deskmate-service status
//! ```
//!
//! A build with the `test-hooks` feature adds the `console` command and the
//! options that keep a test run away from the real service: its data folder,
//! its keys, its pipes and the power commands.

#[cfg(feature = "test-hooks")]
use std::path::PathBuf;

/// The usage text.
pub const USAGE: &str = "\
Deskmate service: keeps this computer connected to Home Assistant while nobody is signed in.

Usage:
  deskmate-service install [--provision-pipe <name>]
      Install or update the service and start it. Needs an elevated terminal.
      With --provision-pipe the settings are read from the one-time pipe
      \\\\.\\pipe\\<name> that the Deskmate app offers.
  deskmate-service uninstall
      Remove the service, its settings, its keys and its folders. Needs an
      elevated terminal.
  deskmate-service status
      Print the status line of the running service.
  deskmate-service run
      What the service manager starts. It does not work from a terminal.

Exit codes: 0 done, 1 failed, 2 wrong usage, 5 an elevated terminal is needed,
6 installed, but the settings were not taken: the program offering them could
not be checked.";

/// Options of `run` and `console`. All of them are test hooks.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// Use this folder instead of the data folder, without checking who can
    /// write to it.
    #[cfg(feature = "test-hooks")]
    pub data_dir: Option<PathBuf>,
    /// Read the pairing key from this file instead of the credential vault.
    #[cfg(feature = "test-hooks")]
    pub key_file: Option<PathBuf>,
    /// Record power commands instead of running them. A build with test
    /// hooks does not start without it.
    #[cfg(feature = "test-hooks")]
    pub dry_run_power: bool,
    /// Add this to the names of both pipes.
    #[cfg(feature = "test-hooks")]
    pub pipe_suffix: Option<String>,
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(RunOptions),
    /// Runs the service in the foreground and takes its events from stdin.
    #[cfg(feature = "test-hooks")]
    Console(RunOptions),
    Install {
        provision_pipe: Option<String>,
        /// Register the service with `run --dry-run-power`.
        #[cfg(feature = "test-hooks")]
        dry_run_power: bool,
        /// Expect this program instead of the app behind the provisioning
        /// pipe.
        #[cfg(feature = "test-hooks")]
        provision_program: Option<String>,
    },
    Uninstall,
    Status {
        #[cfg(feature = "test-hooks")]
        pipe_suffix: Option<String>,
    },
    Help,
    Version,
}

/// The value that follows an option.
fn value<'a>(option: &str, rest: &mut impl Iterator<Item = &'a String>) -> Result<String, String> {
    rest.next()
        .cloned()
        .ok_or_else(|| format!("{option} needs a value"))
}

fn unknown(command: &str, option: &str) -> String {
    format!("`{command}` has no option {option}")
}

/// A release build takes no options here.
#[cfg(not(feature = "test-hooks"))]
fn run_options<'a>(
    command: &str,
    mut rest: impl Iterator<Item = &'a String>,
) -> Result<RunOptions, String> {
    match rest.next() {
        None => Ok(RunOptions::default()),
        Some(option) => Err(unknown(command, option)),
    }
}

#[cfg(feature = "test-hooks")]
fn run_options<'a>(
    command: &str,
    mut rest: impl Iterator<Item = &'a String>,
) -> Result<RunOptions, String> {
    let mut options = RunOptions::default();
    while let Some(option) = rest.next() {
        match option.as_str() {
            "--data-dir" => options.data_dir = Some(value(option, &mut rest)?.into()),
            "--key-file" => options.key_file = Some(value(option, &mut rest)?.into()),
            "--dry-run-power" => options.dry_run_power = true,
            "--pipe-suffix" => options.pipe_suffix = Some(value(option, &mut rest)?),
            other => return Err(unknown(command, other)),
        }
    }
    Ok(options)
}

/// A release build takes no options here.
#[cfg(not(feature = "test-hooks"))]
fn status_command<'a>(mut rest: impl Iterator<Item = &'a String>) -> Result<Command, String> {
    match rest.next() {
        None => Ok(Command::Status {}),
        Some(option) => Err(unknown("status", option)),
    }
}

#[cfg(feature = "test-hooks")]
fn status_command<'a>(mut rest: impl Iterator<Item = &'a String>) -> Result<Command, String> {
    let mut pipe_suffix = None;
    while let Some(option) = rest.next() {
        match option.as_str() {
            "--pipe-suffix" => pipe_suffix = Some(value(option, &mut rest)?),
            other => return Err(unknown("status", other)),
        }
    }
    Ok(Command::Status { pipe_suffix })
}

/// Reads the arguments after the program's name.
pub fn parse(arguments: &[String]) -> Result<Command, String> {
    let mut rest = arguments.iter();
    let Some(command) = rest.next() else {
        return Err("no command given".into());
    };
    match command.as_str() {
        "run" => Ok(Command::Run(run_options("run", rest)?)),
        #[cfg(feature = "test-hooks")]
        "console" => Ok(Command::Console(run_options("console", rest)?)),
        "install" => {
            let mut provision_pipe = None;
            #[cfg(feature = "test-hooks")]
            let mut dry_run_power = false;
            #[cfg(feature = "test-hooks")]
            let mut provision_program = None;
            while let Some(option) = rest.next() {
                match option.as_str() {
                    "--provision-pipe" => provision_pipe = Some(value(option, &mut rest)?),
                    #[cfg(feature = "test-hooks")]
                    "--dry-run-power" => dry_run_power = true,
                    #[cfg(feature = "test-hooks")]
                    "--provision-program" => {
                        provision_program = Some(value(option, &mut rest)?);
                    }
                    other => return Err(unknown("install", other)),
                }
            }
            Ok(Command::Install {
                provision_pipe,
                #[cfg(feature = "test-hooks")]
                dry_run_power,
                #[cfg(feature = "test-hooks")]
                provision_program,
            })
        }
        "uninstall" => match rest.next() {
            None => Ok(Command::Uninstall),
            Some(option) => Err(unknown("uninstall", option)),
        },
        "status" => status_command(rest),
        "help" | "--help" | "-h" => Ok(Command::Help),
        "version" | "--version" | "-V" => Ok(Command::Version),
        other => Err(format!("unknown command {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(words: &[&str]) -> Result<Command, String> {
        let arguments: Vec<String> = words.iter().map(|word| word.to_string()).collect();
        parse(&arguments)
    }

    #[test]
    fn reads_the_commands_of_a_release_build() {
        assert_eq!(
            parse_words(&["run"]),
            Ok(Command::Run(RunOptions::default()))
        );
        assert_eq!(parse_words(&["uninstall"]), Ok(Command::Uninstall));
        assert!(matches!(
            parse_words(&["status"]),
            Ok(Command::Status { .. })
        ));
        assert_eq!(parse_words(&["--help"]), Ok(Command::Help));
        assert_eq!(parse_words(&["--version"]), Ok(Command::Version));

        let Ok(Command::Install { provision_pipe, .. }) =
            parse_words(&["install", "--provision-pipe", "deskmate-provision-1"])
        else {
            panic!("install must be read");
        };
        assert_eq!(provision_pipe.as_deref(), Some("deskmate-provision-1"));
        let Ok(Command::Install { provision_pipe, .. }) = parse_words(&["install"]) else {
            panic!("install must be read");
        };
        assert_eq!(provision_pipe, None);
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert!(parse_words(&[]).is_err());
        assert!(parse_words(&["start"]).is_err());
        assert!(parse_words(&["run", "--verbose"]).is_err());
        assert!(parse_words(&["install", "--provision-pipe"]).is_err());
        assert!(parse_words(&["install", "--force"]).is_err());
        assert!(parse_words(&["uninstall", "--keep-keys"]).is_err());
        assert!(parse_words(&["status", "--json"]).is_err());
    }

    /// A release build cannot be pointed at another folder, another key or
    /// another executor, and has no foreground mode.
    #[cfg(not(feature = "test-hooks"))]
    #[test]
    fn a_release_build_has_no_test_hooks() {
        assert!(parse_words(&["console"]).is_err());
        for option in ["--data-dir", "--key-file", "--pipe-suffix"] {
            assert!(parse_words(&["run", option, "x"]).is_err(), "{option}");
        }
        assert!(parse_words(&["run", "--dry-run-power"]).is_err());
        assert!(parse_words(&["install", "--dry-run-power"]).is_err());
        assert!(parse_words(&["install", "--provision-program", "x.exe"]).is_err());
        assert!(parse_words(&["status", "--pipe-suffix", "x"]).is_err());
        assert!(!USAGE.contains("dry-run"));
    }

    #[cfg(feature = "test-hooks")]
    #[test]
    fn a_test_build_reads_the_test_hooks() {
        let expected = RunOptions {
            data_dir: Some("C:\\temp\\data".into()),
            key_file: Some("C:\\temp\\pairing.key".into()),
            dry_run_power: true,
            pipe_suffix: Some("e2e".into()),
        };
        let words = [
            "console",
            "--data-dir",
            "C:\\temp\\data",
            "--key-file",
            "C:\\temp\\pairing.key",
            "--dry-run-power",
            "--pipe-suffix",
            "e2e",
        ];
        assert_eq!(parse_words(&words), Ok(Command::Console(expected)));
        assert_eq!(
            parse_words(&["run", "--dry-run-power"]),
            Ok(Command::Run(RunOptions {
                dry_run_power: true,
                ..RunOptions::default()
            }))
        );
        assert_eq!(
            parse_words(&[
                "install",
                "--dry-run-power",
                "--provision-pipe",
                "p",
                "--provision-program",
                "python.exe"
            ]),
            Ok(Command::Install {
                provision_pipe: Some("p".into()),
                dry_run_power: true,
                provision_program: Some("python.exe".into()),
            })
        );
        assert_eq!(
            parse_words(&["status", "--pipe-suffix", "e2e"]),
            Ok(Command::Status {
                pipe_suffix: Some("e2e".into())
            })
        );
        assert!(parse_words(&["run", "--data-dir"]).is_err());
    }
}
