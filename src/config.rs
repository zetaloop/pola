use std::{
    collections::HashSet,
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::{mode::Mode, schedule::Schedule};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub shortcut: String,
    pub schedule: Schedule,
    pub profiles: Vec<Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shortcut: "ctrl+shift+alt+d".into(),
            schedule: Schedule::default(),
            profiles: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = path()?;
        match fs::read_to_string(path) {
            Ok(text) => {
                let config: Self = toml::from_str(&text)?;
                config.validate()?;
                Ok(config)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        let path = path()?;
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.name == name)
    }

    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for profile in &self.profiles {
            if profile.name.trim().is_empty() {
                return Err("Enter a configuration name.".into());
            }
            if !names.insert(&profile.name) {
                return Err(
                    format!("A configuration named {:?} already exists.", profile.name).into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<Mode>,
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
