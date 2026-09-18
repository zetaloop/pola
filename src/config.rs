use std::{env, error::Error, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{mode::Mode, schedule::Schedule};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub shortcut: String,
    pub schedule: Schedule,
    pub light: Profile,
    pub dark: Profile,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shortcut: "ctrl+shift+alt+d".into(),
            schedule: Schedule::default(),
            light: Profile::default(),
            dark: Profile::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = path()?;
        match fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn profile(&self, mode: Mode) -> &Profile {
        match mode {
            Mode::Light => &self.light,
            Mode::Dark => &self.dark,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Command {
    pub program: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

impl Command {
    pub fn run(&self) -> io::Result<()> {
        let mut command = std::process::Command::new(&self.program);
        command.args(&self.args);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;

            command.creation_flags(CREATE_NO_WINDOW.0);
        }

        let mut child = command.spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

pub fn path() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base =
        env::var_os("LOCALAPPDATA").ok_or_else(|| io::Error::other("LOCALAPPDATA is not set"))?;

    #[cfg(target_os = "macos")]
    let base = env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    compile_error!("pola supports Windows and macOS");

    #[cfg(target_os = "windows")]
    return Ok(PathBuf::from(base).join("pola").join("config.toml"));

    #[cfg(target_os = "macos")]
    return Ok(PathBuf::from(base).join("Library/Application Support/pola/config.toml"));
}
