pub mod config;
pub mod mode;
pub mod schedule;
pub mod shortcut;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;
