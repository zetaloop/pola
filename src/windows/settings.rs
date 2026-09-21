use std::{rc::Rc, str::FromStr};

use global_hotkey::hotkey::HotKey;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows_reactor::*;

use super::AppState;
use crate::{
    config::Config,
    locale::{Locale, tr},
};

#[derive(Clone)]
pub(crate) struct SettingsInput {
    pub state: Rc<AppState>,
    pub config: Config,
    pub active: bool,
    pub focused: bool,
}

impl PartialEq for SettingsInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
            && self.config == other.config
            && self.active == other.active
            && self.focused == other.focused
    }
}

pub(crate) struct Settings {
    state: Rc<AppState>,
    recording: bool,
    pending: Option<(VirtualKey, String)>,
    error: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Language(Option<usize>),
    Launch(bool),
    Record,
    Pressed(KeyEventInfo),
    Released(KeyEventInfo),
    Cancel,
    Remove,
    ClearError,
}

impl Component for Settings {
    type Input = SettingsInput;
    type Message = Message;

    fn create(input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self {
            state: Rc::clone(&input.state),
            recording: false,
            pending: None,
            error: String::new(),
        }
    }

    fn input_changed(&mut self, input: &Self::Input, _context: &ComponentContext<Self>) {
        if !input.active || !input.focused {
            self.finish();
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        if !matches!(message, Message::Released(_) | Message::Cancel) {
            self.error.clear();
        }
        match message {
            Message::Language(Some(index)) => {
                let mut config = self.state.config();
                config.language = match index {
                    1 => Some(Locale::English),
                    2 => Some(Locale::Chinese),
                    _ => None,
                };
                if let Err(error) = self.state.save_config(config, self.state.launch_at_login()) {
                    self.error = error;
                }
            }
            Message::Language(None) => {}
            Message::Launch(value) => {
                if let Err(error) = self.state.save_config(self.state.config(), value) {
                    self.error = error;
                }
            }
            Message::Record => {
                if !self.recording {
                    match self.state.register_hotkey("") {
                        Ok(()) => {
                            self.recording = true;
                            self.pending = None;
                        }
                        Err(error) => self.error = error,
                    }
                }
            }
            Message::Pressed(info) => {
                if self.recording {
                    match shortcut(info) {
                        Ok(Some(value)) => self.pending = Some((info.original_key, value)),
                        Ok(None) => {}
                        Err(error) => {
                            self.pending = None;
                            self.error = error;
                        }
                    }
                }
            }
            Message::Released(info) => {
                if self.recording
                    && let Some((key, shortcut)) = &self.pending
                    && *key == info.original_key
                {
                    self.error.clear();
                    let mut config = self.state.config();
                    config.shortcut = shortcut.clone();
                    match self.state.save_config(config, self.state.launch_at_login()) {
                        Ok(()) => {
                            self.recording = false;
                            self.pending = None;
                        }
                        Err(error) => self.error = error,
                    }
                }
            }
            Message::Cancel => self.finish(),
            Message::Remove => {
                let mut config = self.state.config();
                config.shortcut.clear();
                if let Err(error) = self.state.save_config(config, self.state.launch_at_login()) {
                    self.error = error;
                }
            }
            Message::ClearError => {}
        }
    }

    fn view(&self, input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        if !input.active {
            return View::empty();
        }
        let recording = self.recording;
        let label = if recording {
            self.pending
                .as_ref()
                .map(|(_, text)| text.as_str())
                .unwrap_or(tr!("Press a key combination"))
        } else if input.config.shortcut.is_empty() {
            tr!("Record shortcut")
        } else {
            &input.config.shortcut
        };
        let cancel = if recording {
            Button::new()
                .grid_column(1)
                .on_click(context.message(Message::Cancel))
                .content(tr!("Cancel"))
        } else {
            View::empty()
        };
        let clear = if !recording {
            Button::new()
                .grid_column(2)
                .is_enabled(!input.config.shortcut.is_empty())
                .on_click(context.message(Message::Remove))
                .content(tr!("Clear"))
        } else {
            View::empty()
        };
        let shortcut = Border::new()
            .on_preview_key_down(context.routed_callback(move |info: KeyEventInfo| {
                if !recording || info.key == VirtualKey::TAB {
                    RoutedMessage::bubble_without_message()
                } else if info.key == VirtualKey::ESCAPE && info.modifiers == InputModifiers::NONE {
                    RoutedMessage::handled(Message::Cancel)
                } else {
                    RoutedMessage::handled(Message::Pressed(info))
                }
            }))
            .on_key_up(context.routed_callback(move |info| {
                if recording {
                    RoutedMessage::handled(Message::Released(info))
                } else {
                    RoutedMessage::bubble_without_message()
                }
            }))
            .on_lost_focus(context.callback(|_| Message::Cancel))
            .content(
                Grid::new()
                    .columns([GridLength::STAR, GridLength::Auto, GridLength::Auto])
                    .column_spacing(8.0)
                    .children((
                        Button::new()
                            .automation_name(tr!("Record global shortcut"))
                            .on_click(context.message(Message::Record))
                            .content(
                                TextBlock::new()
                                    .text(label)
                                    .text_wrapping(TextWrapping::Wrap),
                            ),
                        cancel,
                        clear,
                    )),
            );
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(24.0).content(
                    StackPanel::new().spacing(20.0).children((
                        TextBlock::new()
                            .text(tr!("Settings"))
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        ComboBox::new()
                            .header(tr!("Language"))
                            .items_source([tr!("System default"), "English", "简体中文"])
                            .selected_index(match input.config.language {
                                None => 0,
                                Some(Locale::English) => 1,
                                Some(Locale::Chinese) => 2,
                            })
                            .on_selection_changed(context.callback(Message::Language)),
                        ToggleSwitch::new()
                            .header(tr!("Launch at login"))
                            .is_on(self.state.launch_at_login())
                            .on_toggled(context.callback(Message::Launch)),
                        TextBlock::new()
                            .text(tr!("Global shortcut"))
                            .font_size(20.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        shortcut,
                        InfoBar::new()
                            .is_open(!self.error.is_empty())
                            .severity(InfoBarSeverity::Error)
                            .message(self.error.clone())
                            .on_closed(context.message(Message::ClearError)),
                    )),
                ),
            )
    }
}

impl Settings {
    fn finish(&mut self) {
        if self.recording {
            match self.state.register_hotkey(&self.state.config().shortcut) {
                Ok(()) => {
                    self.recording = false;
                    self.pending = None;
                }
                Err(error) => {
                    if !self.error.is_empty() {
                        self.error.push('\n');
                    }
                    self.error.push_str(&tr!(
                        "Could not restore the shortcut: {error}",
                        error = error
                    ));
                }
            }
        }
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        if self.recording
            && let Err(error) = self.state.register_hotkey(&self.state.config().shortcut)
        {
            super::show_error(tr!("Could not restore shortcut"), &error.to_string());
        }
    }
}

fn shortcut(info: KeyEventInfo) -> Result<Option<String>, String> {
    let key = VIRTUAL_KEY(info.original_key.0 as u16);
    let key = match key {
        VK_SHIFT | VK_CONTROL | VK_MENU | VK_LSHIFT | VK_RSHIFT | VK_LCONTROL | VK_RCONTROL
        | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN => return Ok(None),
        key if (VK_A.0..=VK_Z.0).contains(&key.0) || (VK_0.0..=VK_9.0).contains(&key.0) => {
            char::from_u32(u32::from(key.0)).unwrap().to_string()
        }
        key if (VK_F1.0..=VK_F24.0).contains(&key.0) => format!("F{}", key.0 - VK_F1.0 + 1),
        key if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&key.0) => {
            format!("Numpad{}", key.0 - VK_NUMPAD0.0)
        }
        key => match key {
            VK_BACK => "Backspace",
            VK_TAB => "Tab",
            VK_SPACE => "Space",
            VK_RETURN if info.status.is_extended => "NumpadEnter",
            VK_RETURN => "Enter",
            VK_ESCAPE => "Escape",
            VK_CAPITAL => "CapsLock",
            VK_PRIOR => "PageUp",
            VK_NEXT => "PageDown",
            VK_HOME => "Home",
            VK_END => "End",
            VK_LEFT => "Left",
            VK_RIGHT => "Right",
            VK_UP => "Up",
            VK_DOWN => "Down",
            VK_INSERT => "Insert",
            VK_DELETE => "Delete",
            VK_SNAPSHOT => "PrintScreen",
            VK_NUMLOCK => "NumLock",
            VK_SCROLL => "ScrollLock",
            VK_PAUSE => "Pause",
            VK_ADD => "NumpadAdd",
            VK_SUBTRACT => "NumpadSubtract",
            VK_MULTIPLY => "NumpadMultiply",
            VK_DIVIDE => "NumpadDivide",
            VK_DECIMAL => "NumpadDecimal",
            VK_OEM_PLUS => "Equal",
            VK_OEM_MINUS => "Minus",
            VK_OEM_COMMA => "Comma",
            VK_OEM_PERIOD => "Period",
            VK_OEM_1 => "Semicolon",
            VK_OEM_2 => "Slash",
            VK_OEM_3 => "Backquote",
            VK_OEM_4 => "BracketLeft",
            VK_OEM_5 => "Backslash",
            VK_OEM_6 => "BracketRight",
            VK_OEM_7 => "Quote",
            VK_VOLUME_DOWN => "AudioVolumeDown",
            VK_VOLUME_UP => "AudioVolumeUp",
            VK_VOLUME_MUTE => "AudioVolumeMute",
            VK_PLAY => "MediaPlay",
            VK_MEDIA_PLAY_PAUSE => "MediaPlayPause",
            VK_MEDIA_STOP => "MediaStop",
            VK_MEDIA_NEXT_TRACK => "MediaTrackNext",
            VK_MEDIA_PREV_TRACK => "MediaTrackPrevious",
            _ => return Err(tr!("This key cannot be used as a global shortcut.").into()),
        }
        .into(),
    };
    let mut parts = Vec::new();
    for (modifier, name) in [
        (InputModifiers::CONTROL, "ctrl"),
        (InputModifiers::ALT, "alt"),
        (InputModifiers::SHIFT, "shift"),
        (InputModifiers::WINDOWS, "super"),
    ] {
        if info.modifiers.contains(modifier) {
            parts.push(name.to_owned());
        }
    }
    parts.push(key);
    HotKey::from_str(&parts.join("+"))
        .map(|hotkey| Some(hotkey.into_string()))
        .map_err(|error| error.to_string())
}
