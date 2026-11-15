//! Deskmate core: the code the desktop app shares with programs that run
//! without a window. Nothing in this crate may depend on Tauri.

pub mod config;
pub mod consts;
pub mod cpu_temperature;
pub mod custom_sensors;
pub mod discovery;
pub mod hardware;
pub mod link;
pub mod machine;
pub mod seat;
pub mod secrets;
pub mod security;
pub mod sensors;
pub mod sys_commands;
#[cfg(all(test, unix))]
mod test_tree;
