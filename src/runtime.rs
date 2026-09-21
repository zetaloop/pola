use std::cell::{Cell, RefCell};

#[cfg(target_os = "macos")]
use crate::macos::system;
#[cfg(target_os = "windows")]
use crate::windows::system;
use crate::{config::Config, mode::Mode, schedule::Event, shortcut::Shortcut};

pub struct Runtime {
    pub config: RefCell<Config>,
    pub shortcut: RefCell<Shortcut>,
    pub next: RefCell<Option<Event>>,
    applied: Cell<Option<Mode>>,
}

impl Runtime {
    pub fn new(config: Config) -> Self {
        Self {
            config: RefCell::new(config),
            shortcut: RefCell::new(Shortcut::default()),
            next: RefCell::new(None),
            applied: Cell::new(None),
        }
    }

    pub fn save(&self, config: Config) -> Result<(), String> {
        self.shortcut
            .borrow_mut()
            .register(&config.shortcut)
            .map_err(|error| error.to_string())?;
        if let Err(error) = config.save() {
            if let Err(restore) = self
                .shortcut
                .borrow_mut()
                .register(&self.config.borrow().shortcut)
            {
                return Err(format!("{error}\nShortcut: {restore}"));
            }
            return Err(error.to_string());
        }
        *self.config.borrow_mut() = config;
        Ok(())
    }

    pub fn apply(&self, mode: Mode) -> Result<(), String> {
        if self.applied.replace(Some(mode)) == Some(mode) {
            return Ok(());
        }

        let profile = self.config.borrow().profile(mode).clone();
        let mut errors = Vec::new();
        if let Some(path) = &profile.wallpaper
            && let Err(error) = system::set_wallpaper(path)
        {
            errors.push(format!("Wallpaper: {error}"));
        }
        for command in &profile.commands {
            if let Err(error) = command.run() {
                errors.push(format!("{}: {error}", command.program));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}
