#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
fn main() {
    pola::macos::run();
}

#[cfg(target_os = "windows")]
fn main() {
    pola::windows::run();
}
