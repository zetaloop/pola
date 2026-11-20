use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::mode::Mode;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub light: Profile,
    pub dark: Profile,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|error| error.to_string()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn profile(&self, mode: Mode) -> &Profile {
        match mode {
            Mode::Light => &self.light,
            Mode::Dark => &self.dark,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub wallpaper: Option<PathBuf>,
    pub commands: Vec<Command>,
}

#[derive(Debug, Deserialize)]
pub struct Command {
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_appearance_profiles() {
        let config: Config = toml::from_str(
            r#"
            [light]
            wallpaper = "Wallpapers/Day.jpg"
            commands = [{ program = "wallpaper64.exe", args = ["-control", "openProfile", "-profile", "Light"] }]

            [dark]
            wallpaper = "Wallpapers/Night.jpg"
            "#,
        )
        .unwrap();

        assert_eq!(
            config.light.wallpaper.as_deref(),
            Some(Path::new("Wallpapers/Day.jpg"))
        );
        assert_eq!(
            config.light.commands[0].args,
            ["-control", "openProfile", "-profile", "Light"]
        );
        assert_eq!(
            config.dark.wallpaper.as_deref(),
            Some(Path::new("Wallpapers/Night.jpg"))
        );
    }
}
