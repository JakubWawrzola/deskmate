//! The displays: whether the screen is on, waking it, and its brightness.

#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "the screen state, wake and brightness use Windows APIs; elsewhere only the stubs compile"
    )
)]

pub mod brightness;
mod brightness_win;
pub mod screen_state;
mod wake;

pub use wake::wake;
