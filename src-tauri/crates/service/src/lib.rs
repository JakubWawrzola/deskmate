//! Deskmate without a signed-in user: the part of a computer that Home
//! Assistant can see and reach before anyone has signed in.
//!
//! Everything here except `win` and `linux` is platform-neutral. `runner` is
//! the whole behaviour; the platform layer feeds it events and serves its
//! status. The Windows layer (`win`) is the service wrapper, the two named
//! pipes, the access checks and the install and uninstall steps. The Linux
//! layer (`linux`) is the daemon that systemd starts: its signals, its status
//! file and the commands that pair and unpair it.

#![deny(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod admin;
pub mod audit;
pub mod cli;
pub mod config;
pub mod connector;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod logging;
pub mod pairing;
pub mod power;
pub mod provisioning;
pub mod runner;
pub mod secrets;
pub mod status;
#[cfg(windows)]
pub mod win;

/// Name of the service's folder inside the system's data folder
/// (`%ProgramData%\Deskmate\service` on Windows).
pub const DATA_FOLDER: [&str; 2] = ["Deskmate", "service"];
