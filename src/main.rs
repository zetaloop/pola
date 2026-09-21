#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod ipc;
mod mode;
mod runtime;
mod schedule;
mod shortcut;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

fn main() -> std::process::ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let daemon = args.first().is_some_and(|arg| arg == "daemon");
    let ready = args.get(1).is_some_and(|arg| arg == "--ready");
    let result = match args.as_slice() {
        [] => platform::run(),
        [entry] if entry == "daemon" => platform::daemon::run(false),
        [entry, flag] if entry == "daemon" && flag == "--ready" => platform::daemon::run(true),
        _ => Err("Usage: pola [daemon]".into()),
    };
    if let Err(error) = result {
        if daemon {
            if ready {
                _ = ipc::write(&mut std::io::stdout(), &Err::<(), _>(&error));
            }
            eprintln!("{error}");
        } else {
            platform::show_error("Could not start pola", &error);
        }
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
