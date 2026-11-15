//! The displays: whether the screen is on, waking it, and its brightness.

pub mod screen_state;
mod wake;

pub use wake::wake;
