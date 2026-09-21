use std::{error::Error, str::FromStr};

use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};

use crate::locale::tr;

#[derive(Default)]
pub struct Shortcut {
    manager: Option<GlobalHotKeyManager>,
    hotkey: Option<HotKey>,
}

impl Shortcut {
    pub fn register(&mut self, text: &str) -> Result<(), Box<dyn Error>> {
        if text.is_empty() {
            if let Some(old) = self.hotkey {
                self.manager.as_ref().unwrap().unregister(old)?;
                self.hotkey = None;
            }
            return Ok(());
        }
        let hotkey = HotKey::from_str(text)?;
        if self.hotkey == Some(hotkey) {
            return Ok(());
        }

        if self.manager.is_none() {
            self.manager = Some(GlobalHotKeyManager::new()?);
        }
        let manager = self.manager.as_ref().unwrap();
        let old = self.hotkey;

        if let Some(old) = old {
            manager.unregister(old)?;
        }

        if let Err(error) = manager.register(hotkey) {
            self.hotkey = None;
            if let Some(old) = old {
                manager.register(old).map_err(|restore| {
                    tr!(
                        "{error}\nShortcut: {restore}",
                        error = error,
                        restore = restore
                    )
                })?;
                self.hotkey = Some(old);
            }
            return Err(error.into());
        }

        self.hotkey = Some(hotkey);
        Ok(())
    }

    pub fn matches(&self, id: u32) -> bool {
        self.hotkey.is_some_and(|hotkey| hotkey.id() == id)
    }
}
