#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("pola supports Windows only");

mod config;
mod mode;
mod windows;

use std::{
    fmt::Display,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Paths {
    data: PathBuf,
    config: PathBuf,
    log: PathBuf,
}

impl Paths {
    fn new() -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or_else(|| io::Error::other("executable directory is unavailable"))?;
        let data = directory.join("data");
        fs::create_dir_all(&data)?;
        Ok(Self {
            config: data.join("config.toml"),
            log: data.join("pola.log"),
            data,
        })
    }
}

pub(crate) fn report(path: &Path, message: impl Display) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        _ = writeln!(file, "{timestamp} {message}");
    }
}

fn run(paths: &Paths) -> Result<(), String> {
    std::env::set_current_dir(&paths.data).map_err(|error| error.to_string())?;
    let config = config::Config::load(&paths.config)?;
    windows::run(&config, &paths.data, &paths.log)
}

fn main() {
    let paths = match Paths::new() {
        Ok(paths) => paths,
        Err(_) => return,
    };
    if let Err(error) = run(&paths) {
        report(&paths.log, error);
    }
}
