//! Deskmate core: the code the desktop app shares with programs that run
//! without a window. Nothing in this crate may depend on Tauri.

pub mod config;
pub mod consts;
pub mod cpu_temperature;
pub mod hardware;
pub mod security;
pub mod sensors;
