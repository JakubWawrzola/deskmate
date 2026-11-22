//! Deskmate core: the code the desktop app shares with programs that run
//! without a window. Nothing in this crate may depend on Tauri.
//! It builds for Windows and for Linux.

pub mod config;
pub mod console_user;
pub mod consts;
#[cfg(windows)]
pub mod cpu_temperature;
pub mod custom_sensors;
pub mod discovery;
pub mod hardware;
pub mod link;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod machine;
pub mod seat;
pub mod secrets;
pub mod security;
pub mod sensors;
pub mod sys_commands;
#[cfg(all(test, unix))]
mod test_tree;
