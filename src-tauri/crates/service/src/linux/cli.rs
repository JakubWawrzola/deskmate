//! The command line of the Linux daemon.
//!
//! ```text
//! deskmate-service run
//! deskmate-service pair
//! deskmate-service unpair
//! deskmate-service status
//! ```
//!
//! A build with the `test-hooks` feature adds the `console` command and the
//! options that keep a test run away from the real daemon: its folders, its
//! status file and the power commands.

#[cfg(feature = "test-hooks")]
use std::path::PathBuf;

/// The usage text.
pub const USAGE: &str = "\
Deskmate daemon: keeps this computer connected to Home Assistant, with its sensors and power buttons.

Usage:
  deskmate-service pair
      Pair the daemon with Home Assistant. Paste the pairing code from the
      Deskmate integration when asked; it is read from standard input, never
      from the command line. Needs root: sudo deskmate-service pair
  deskmate-service unpair
      Forget the settings and the keys. Needs root.
  deskmate-service status
      Print the daemon's status.
  deskmate-service run
      What systemd starts (deskmate.service).
  deskmate-service --version
  deskmate-service --help

Exit codes: 0 done, 1 failed, 2 wrong usage, 5 root is needed.";

/// Options of every command. All of them are test hooks.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Use this folder instead of `/etc/deskmate`.
    #[cfg(feature = "test-hooks")]
    pub config_dir: Option<PathBuf>,
    /// Use this folder instead of `/var/lib/deskmate`.
    #[cfg(feature = "test-hooks")]
    pub data_dir: Option<PathBuf>,
    /// Use this file instead of `/run/deskmate/status.json`.
    #[cfg(feature = "test-hooks")]
    pub status_file: Option<PathBuf>,
    /// Record power commands instead of running them. `run` and `console` in
    /// a build with test hooks do not start without it.
    #[cfg(feature = "test-hooks")]
    pub dry_run_power: bool,
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    /// The daemon in the foreground; its events also come from stdin.
    #[cfg(feature = "test-hooks")]
    Console(Options),
    Pair(Options),
    Unpair(Options),
    Status(Options),
    Help,
    Version,
}

/// Which options a command takes in a build with test hooks.
#[cfg_attr(
    not(feature = "test-hooks"),
    allow(dead_code, reason = "a release build takes no options")
)]
#[derive(Clone, Copy)]
struct Accepts {
    folders: bool,
    status_file: bool,
    dry_run_power: bool,
}

const FOR_RUN: Accepts = Accepts {
    folders: true,
    status_file: true,
    dry_run_power: true,
};
const FOR_PAIRING: Accepts = Accepts {
    folders: true,
    status_file: false,
    dry_run_power: false,
};
const FOR_STATUS: Accepts = Accepts {
    folders: false,
    status_file: true,
    dry_run_power: false,
};

fn unknown(command: &str, option: &str) -> String {
    format!("`{command}` has no option {option}")
}

/// A release build takes no options.
#[cfg(not(feature = "test-hooks"))]
fn options<'a>(
    command: &str,
    _accepts: Accepts,
    mut rest: impl Iterator<Item = &'a String>,
) -> Result<Options, String> {
    match rest.next() {
        None => Ok(Options::default()),
        Some(option) => Err(unknown(command, option)),
    }
}

#[cfg(feature = "test-hooks")]
fn options<'a>(
    command: &str,
    accepts: Accepts,
    mut rest: impl Iterator<Item = &'a String>,
) -> Result<Options, String> {
    let mut parsed = Options::default();
    while let Some(option) = rest.next() {
        match option.as_str() {
            "--config-dir" if accepts.folders => {
                parsed.config_dir = Some(path_value(option, &mut rest)?);
            }
            "--data-dir" if accepts.folders => {
                parsed.data_dir = Some(path_value(option, &mut rest)?);
            }
            "--status-file" if accepts.status_file => {
                parsed.status_file = Some(path_value(option, &mut rest)?);
            }
            "--dry-run-power" if accepts.dry_run_power => parsed.dry_run_power = true,
            other => return Err(unknown(command, other)),
        }
    }
    Ok(parsed)
}

/// The word after an option, which it needs as its value.
#[cfg(feature = "test-hooks")]
fn path_value<'a>(
    option: &str,
    rest: &mut impl Iterator<Item = &'a String>,
) -> Result<PathBuf, String> {
    rest.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{option} needs a value"))
}

/// Reads the arguments after the program's name.
pub fn parse(arguments: &[String]) -> Result<Command, String> {
    let mut rest = arguments.iter();
    let Some(command) = rest.next() else {
        return Err("no command given".into());
    };
    match command.as_str() {
        "run" => Ok(Command::Run(options("run", FOR_RUN, rest)?)),
        #[cfg(feature = "test-hooks")]
        "console" => Ok(Command::Console(options("console", FOR_RUN, rest)?)),
        "pair" => Ok(Command::Pair(options("pair", FOR_PAIRING, rest)?)),
        "unpair" => Ok(Command::Unpair(options("unpair", FOR_PAIRING, rest)?)),
        "status" => Ok(Command::Status(options("status", FOR_STATUS, rest)?)),
        "help" | "--help" | "-h" => match rest.next() {
            None => Ok(Command::Help),
            Some(option) => Err(unknown("help", option)),
        },
        "version" | "--version" | "-V" => match rest.next() {
            None => Ok(Command::Version),
            Some(option) => Err(unknown("version", option)),
        },
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
        assert_eq!(parse_words(&["run"]), Ok(Command::Run(Options::default())));
        assert_eq!(
            parse_words(&["pair"]),
            Ok(Command::Pair(Options::default()))
        );
        assert_eq!(
            parse_words(&["unpair"]),
            Ok(Command::Unpair(Options::default()))
        );
        assert_eq!(
            parse_words(&["status"]),
            Ok(Command::Status(Options::default()))
        );
        assert_eq!(parse_words(&["--help"]), Ok(Command::Help));
        assert_eq!(parse_words(&["-h"]), Ok(Command::Help));
        assert_eq!(parse_words(&["--version"]), Ok(Command::Version));
        assert_eq!(parse_words(&["-V"]), Ok(Command::Version));
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert!(parse_words(&[]).is_err());
        assert!(parse_words(&["start"]).is_err());
        assert!(
            parse_words(&["install"]).is_err(),
            "systemd installs the daemon"
        );
        assert!(parse_words(&["run", "--verbose"]).is_err());
        assert!(parse_words(&["status", "--json"]).is_err());
        assert!(parse_words(&["--version", "now"]).is_err());
    }

    /// The pairing code is read from stdin: an argument would land in the
    /// shell history and in the process list for every user to see.
    #[test]
    fn pair_takes_no_code_on_the_command_line() {
        assert!(parse_words(&["pair", "DMP1.eyJrIjoiQUFBQSJ9"]).is_err());
        assert!(parse_words(&["pair", "--code", "DMP1.x"]).is_err());
    }

    /// A release build cannot be pointed at other folders or another
    /// executor, and has no foreground mode.
    #[cfg(not(feature = "test-hooks"))]
    #[test]
    fn a_release_build_has_no_test_hooks() {
        assert!(parse_words(&["console"]).is_err());
        for option in ["--config-dir", "--data-dir", "--status-file"] {
            assert!(parse_words(&["run", option, "/tmp/x"]).is_err(), "{option}");
            assert!(
                parse_words(&["pair", option, "/tmp/x"]).is_err(),
                "{option}"
            );
        }
        assert!(parse_words(&["run", "--dry-run-power"]).is_err());
        assert!(!USAGE.contains("dry-run"));
    }

    #[cfg(feature = "test-hooks")]
    #[test]
    fn a_test_build_reads_the_test_hooks() {
        let expected = Options {
            config_dir: Some("/tmp/e2e/etc".into()),
            data_dir: Some("/tmp/e2e/state".into()),
            status_file: Some("/tmp/e2e/status.json".into()),
            dry_run_power: true,
        };
        let words = [
            "console",
            "--config-dir",
            "/tmp/e2e/etc",
            "--data-dir",
            "/tmp/e2e/state",
            "--status-file",
            "/tmp/e2e/status.json",
            "--dry-run-power",
        ];
        assert_eq!(parse_words(&words), Ok(Command::Console(expected)));
        assert_eq!(
            parse_words(&["pair", "--config-dir", "/tmp/a", "--data-dir", "/tmp/b"]),
            Ok(Command::Pair(Options {
                config_dir: Some("/tmp/a".into()),
                data_dir: Some("/tmp/b".into()),
                ..Options::default()
            }))
        );
        assert_eq!(
            parse_words(&["status", "--status-file", "/tmp/s.json"]),
            Ok(Command::Status(Options {
                status_file: Some("/tmp/s.json".into()),
                ..Options::default()
            }))
        );
        assert!(parse_words(&["run", "--data-dir"]).is_err());
        assert!(parse_words(&["pair", "--dry-run-power"]).is_err());
        assert!(parse_words(&["pair", "--status-file", "/tmp/s"]).is_err());
        assert!(parse_words(&["status", "--config-dir", "/tmp/a"]).is_err());
    }
}
