#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod mode;
mod runtime;
mod schedule;
mod shortcut;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
}

#[cfg(target_os = "windows")]
fn main() {
    windows::run();
}
