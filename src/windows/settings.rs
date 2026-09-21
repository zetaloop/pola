use std::{rc::Rc, str::FromStr};

use global_hotkey::hotkey::HotKey;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows_reactor::*;

use super::AppState;
use crate::config::Config;

#[derive(Clone)]
pub(crate) struct SettingsInput {
    pub state: Rc<AppState>,
    pub config: Config,
}

impl PartialEq for SettingsInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state) && self.config == other.config
    }
}

pub(crate) struct Settings {
    state: Rc<AppState>,
    recorder: ElementRef<Border>,
    recording: bool,
    pending: Option<String>,
    error: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Launch(bool),
    Record,
    Pressed(KeyEventInfo),
    Released,
    Cancel,
    Remove,
    FocusFailed,
    ClearError,
}

impl Component for Settings {
    type Input = SettingsInput;
    type Message = Message;

    fn create(input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self {
            state: Rc::clone(&input.state),
            recorder: ElementRef::new(),
            recording: false,
            pending: None,
            error: String::new(),
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        if !matches!(message, Message::Released | Message::Cancel) {
            self.error.clear();
        }
        match message {
            Message::Launch(value) => {
                if let Err(error) = self.state.save_config(self.state.config(), value) {
                    self.error = error;
                }
            }
            Message::Record => match self.state.register_hotkey("") {
                Ok(()) => {
                    self.recording = true;
                    self.pending = None;
                }
                Err(error) => self.error = error.to_string(),
            },
            Message::Pressed(info) => {
                if self.recording {
                    match shortcut(info) {
                        Ok(Some(value)) => self.pending = Some(value),
                        Ok(None) => {}
                        Err(error) => {
                            self.pending = None;
                            self.error = error;
                        }
                    }
                }
            }
            Message::Released => {
                if self.recording
                    && let Some(shortcut) = self.pending.take()
                {
                    self.error.clear();
                    let mut config = self.state.config();
                    config.shortcut = shortcut;
                    match self.state.save_config(config, self.state.launch_at_login()) {
                        Ok(()) => self.recording = false,
                        Err(error) => {
                            self.error = error;
                            self.finish();
                        }
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
            Message::FocusFailed => {
                self.error = "Could not focus the shortcut recorder.".into();
                self.finish();
            }
            Message::ClearError => {}
        }
    }

    fn view(&self, input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        if self.recording {
            let recorder = self.recorder.clone();
            let failed = context.message(Message::FocusFailed);
            let completed = failed.clone();
            context.use_effect("shortcut-focus", (), move || {
                if !recorder.request_focus_result(move |result| {
                    if !matches!(result, Ok(true)) {
                        _ = completed.call(());
                    }
                }) {
                    _ = failed.call(());
                }
                None
            });
        }
        let shortcut: View = if self.recording {
            StackPanel::new().spacing(12.0).children((
                Border::new()
                    .element_ref(&self.recorder)
                    .is_tab_stop(true)
                    .focus_on_pointer_release(true)
                    .automation_name("Record global shortcut")
                    .background(ThemeBrush::CardBackground)
                    .border_brush(ThemeBrush::CardStroke)
                    .border_thickness(1.0)
                    .corner_radius(8.0)
                    .padding(20.0)
                    .on_preview_key_down(context.routed_callback(|info: KeyEventInfo| {
                        let modifiers = info.modifiers;
                        if info.key == VirtualKey::TAB
                            && !modifiers.contains(InputModifiers::CONTROL)
                            && !modifiers.contains(InputModifiers::ALT)
                            && !modifiers.contains(InputModifiers::WINDOWS)
                        {
                            RoutedMessage::bubble_without_message()
                        } else if info.key == VirtualKey::ESCAPE
                            && modifiers == InputModifiers::NONE
                        {
                            RoutedMessage::handled(Message::Cancel)
                        } else {
                            RoutedMessage::handled(Message::Pressed(info))
                        }
                    }))
                    .on_key_up(
                        context.routed_callback(|_| RoutedMessage::handled(Message::Released)),
                    )
                    .on_lost_focus(context.callback(|_| Message::Cancel))
                    .content(
                        TextBlock::new()
                            .text(self.pending.as_deref().unwrap_or("Press a key combination")),
                    ),
                Button::new()
                    .on_click(context.message(Message::Cancel))
                    .content("Cancel recording"),
            ))
        } else {
            StackPanel::new().spacing(12.0).children((
                TextBlock::new().text(if input.config.shortcut.is_empty() {
                    "None"
                } else {
                    &input.config.shortcut
                }),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        Button::new()
                            .on_click(context.message(Message::Record))
                            .content("Record shortcut"),
                        Button::new()
                            .is_enabled(!input.config.shortcut.is_empty())
                            .on_click(context.message(Message::Remove))
                            .content("Remove shortcut"),
                    )),
            ))
        };
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(28.0).content(
                    StackPanel::new().spacing(24.0).max_width(800.0).children((
                        TextBlock::new()
                            .text("Settings")
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        ToggleSwitch::new()
                            .header("Launch at login")
                            .is_on(self.state.launch_at_login())
                            .is_enabled(!self.recording)
                            .on_toggled(context.callback(Message::Launch)),
                        TextBlock::new()
                            .text("Global shortcut")
                            .font_size(20.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        TextBlock::new()
                            .text("Switch between light and dark appearance from any application.")
                            .text_wrapping(TextWrapping::Wrap),
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
                    self.error
                        .push_str(&format!("Could not restore the shortcut: {error}"));
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
            super::show_error("Could not restore shortcut", &error.to_string());
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
            _ => return Err("This key cannot be used as a global shortcut.".into()),
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
