#[cfg(target_os = "macos")]
fn main() {
    pola::macos::run();
}

#[cfg(target_os = "windows")]
fn main() {
    pola::windows::run();
}
