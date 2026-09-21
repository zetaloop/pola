use std::{
    collections::{BTreeSet, HashSet},
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::PathBuf,
    process::Stdio,
};

use serde::{Deserialize, Serialize};

use crate::{
    locale::{self, Locale, tr},
    mode::Mode,
    schedule::Schedule,
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Locale>,
    pub shortcut: String,
    pub schedule: Schedule,
    pub profiles: Vec<Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: None,
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
                locale::set(config.language);
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
                return Err(tr!("Enter a configuration name.").into());
            }
            if !names.insert(&profile.name) {
                return Err(tr!(
                    "A configuration named {name:?} already exists.",
                    name = profile.name
                )
                .into());
            }
            for action in &profile.actions {
                action.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub when: BTreeSet<Mode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Color {
        mode: Mode,
    },
    #[cfg(target_os = "windows")]
    Theme {
        path: PathBuf,
    },
    Wallpaper {
        path: PathBuf,
    },
    Command(Command),
}

impl Action {
    pub fn choices() -> Vec<Self> {
        vec![
            Self::Color { mode: Mode::Light },
            Self::Wallpaper {
                path: PathBuf::new(),
            },
            Self::Command(Command::default()),
            #[cfg(target_os = "windows")]
            Self::Theme {
                path: PathBuf::new(),
            },
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Color { .. } => tr!("System appearance"),
            Self::Wallpaper { .. } => tr!("Wallpaper"),
            Self::Command(_) => tr!("Command"),
            #[cfg(target_os = "windows")]
            Self::Theme { .. } => tr!("Windows theme"),
        }
    }

    pub fn summary(&self) -> String {
        let target = match self {
            Self::Color { mode } => mode.label().into(),
            Self::Wallpaper { path } => path.display().to_string(),
            Self::Command(command) => command.program.clone(),
            #[cfg(target_os = "windows")]
            Self::Theme { path } => path.display().to_string(),
        };
        tr!("{title}: {target}", title = self.title(), target = target)
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Command(command) if command.program.is_empty() => {
                Err(tr!("Enter a program to run.").into())
            }
            Self::Wallpaper { path } if path.as_os_str().is_empty() => {
                Err(tr!("Enter a wallpaper path.").into())
            }
            #[cfg(target_os = "windows")]
            Self::Theme { path } if path.as_os_str().is_empty() => {
                Err(tr!("Enter a theme path.").into())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Command {
    pub program: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    pub wait: bool,
}

impl Default for Command {
    fn default() -> Self {
        Self {
            program: String::new(),
            args: Vec::new(),
            wait: true,
        }
    }
}

impl Command {
    pub fn run(&self) -> io::Result<()> {
        let mut command = std::process::Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;

            command.creation_flags(CREATE_NO_WINDOW.0);
        }

        if self.wait {
            let output = command.output()?;
            if output.status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "{}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                )))
            }
        } else {
            let mut child = command.stderr(Stdio::null()).spawn()?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
    }
}

pub fn path() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::other(tr!("{variable} is not set", variable = "LOCALAPPDATA")))?;

    #[cfg(target_os = "macos")]
    let base = env::var_os("HOME")
        .ok_or_else(|| io::Error::other(tr!("{variable} is not set", variable = "HOME")))?;

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    compile_error!("pola supports Windows and macOS");

    #[cfg(target_os = "windows")]
    return Ok(PathBuf::from(base).join("pola").join("config.toml"));

    #[cfg(target_os = "macos")]
    return Ok(PathBuf::from(base).join("Library/Application Support/pola/config.toml"));
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn commands_report_exit_errors() {
        #[cfg(target_os = "macos")]
        let (program, args) = ("/bin/sh", vec!["-c", "printf 'command failed' >&2; exit 7"]);
        #[cfg(target_os = "windows")]
        let (program, args) = (
            "cmd.exe",
            vec!["/d", "/c", "echo command failed 1>&2 & exit /b 7"],
        );
        let error = Command {
            program: program.into(),
            args: args.into_iter().map(String::from).collect(),
            ..Command::default()
        }
        .run()
        .unwrap_err();
        assert!(error.to_string().contains("command failed"));
    }
}
