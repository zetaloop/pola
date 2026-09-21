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
    #[cfg(target_os = "windows")]
    if !args.is_empty() && !daemon {
        use ::windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        unsafe {
            _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    let result = match args.as_slice() {
        [] => platform::run(),
        [entry] if entry == "daemon" => platform::daemon::run(false),
        [entry, flag] if entry == "daemon" && flag == "--ready" => platform::daemon::run(true),
        [entry, name] if entry == "run" => name
            .to_str()
            .ok_or_else(|| "Configuration name must be Unicode.".to_string())
            .and_then(|name| {
                let client = ipc::Client::connect(|error| {
                    if let Some(error) = error {
                        eprintln!("{error}");
                    }
                })?;
                client.request(ipc::Request::Run {
                    name: name.into(),
                    wait: true,
                })
            }),
        _ => Err("Usage: pola [daemon | run NAME]".into()),
    };
    if let Err(error) = result {
        if !args.is_empty() {
            if daemon && ready {
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
