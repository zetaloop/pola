use std::{cell::Cell, str::FromStr};

use global_hotkey::hotkey::HotKey;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};

use super::settings::Settings;

pub struct Ivars {
    owner: Weak<Settings>,
    recording: Cell<bool>,
}

define_class!(
    #[unsafe(super = NSButton)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Recorder;

    unsafe impl NSObjectProtocol for Recorder {}

    impl Recorder {
        #[unsafe(method(record:))]
        fn record(&self, _sender: &NSObject) {
            if let Some(owner) = self.ivars().owner.load()
                && owner.suspend_shortcut()
            {
                self.ivars().recording.set(true);
                self.setTitle(&NSString::from_str("Press a shortcut…"));
                self.window().unwrap().makeFirstResponder(Some(self));
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_focus(&self) -> bool {
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_focus(&self) -> bool {
            self.finish();
            unsafe { msg_send![super(self), resignFirstResponder] }
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn key_equivalent(&self, event: &NSEvent) -> bool {
            if self.ivars().recording.get() {
                self.capture(event);
                true
            } else {
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if self.ivars().recording.get() {
                self.capture(event);
            } else {
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }
    }
);

impl Recorder {
    pub fn new(mtm: MainThreadMarker, owner: &Settings) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            recording: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBezelStyle(NSBezelStyle::Push);
        unsafe {
            this.setTarget(Some(&this));
            this.setAction(Some(sel!(record:)));
        }
        this
    }

    pub fn display(&self, value: &str) {
        if self.ivars().recording.get() {
            return;
        }
        let title = if value.is_empty() {
            "Record shortcut…".into()
        } else {
            value
                .replace("ctrl+", "⌃")
                .replace("control+", "⌃")
                .replace("alt+", "⌥")
                .replace("shift+", "⇧")
                .replace("super+", "⌘")
                .replace("meta+", "⌘")
                .replace("Key", "")
                .replace("Digit", "")
                .to_uppercase()
        };
        self.setTitle(&NSString::from_str(&title));
    }

    fn capture(&self, event: &NSEvent) {
        if event.isARepeat() {
            return;
        }
        let Some(owner) = self.ivars().owner.load() else {
            return;
        };
        let Some(text) = event.charactersByApplyingModifiers(NSEventModifierFlags::empty()) else {
            return;
        };
        let text = text.to_string();
        let Some(character) = text.chars().next() else {
            return;
        };
        if character == '\u{1b}' {
            self.finish();
            return;
        }
        let key = match character as u32 {
            objc2_app_kit::NSUpArrowFunctionKey => "Up".into(),
            objc2_app_kit::NSDownArrowFunctionKey => "Down".into(),
            objc2_app_kit::NSLeftArrowFunctionKey => "Left".into(),
            objc2_app_kit::NSRightArrowFunctionKey => "Right".into(),
            objc2_app_kit::NSHomeFunctionKey => "Home".into(),
            objc2_app_kit::NSEndFunctionKey => "End".into(),
            objc2_app_kit::NSPageUpFunctionKey => "PageUp".into(),
            objc2_app_kit::NSPageDownFunctionKey => "PageDown".into(),
            objc2_app_kit::NSDeleteFunctionKey => "Delete".into(),
            objc2_app_kit::NSInsertFunctionKey => "Insert".into(),
            objc2_app_kit::NSF1FunctionKey..=objc2_app_kit::NSF35FunctionKey => {
                format!("F{}", character as u32 - objc2_app_kit::NSF1FunctionKey + 1)
            }
            _ => match character {
                ' ' => "Space".into(),
                '\r' | '\n' => "Enter".into(),
                '\t' => "Tab".into(),
                '\u{7f}' | '\u{8}' => "Backspace".into(),
                '\u{3}' => "NumpadEnter".into(),
                _ if event
                    .modifierFlags()
                    .contains(NSEventModifierFlags::NumericPad) =>
                {
                    match character {
                        '0'..='9' => format!("Numpad{character}"),
                        '+' => "NumpadAdd".into(),
                        '-' => "NumpadSubtract".into(),
                        '*' => "NumpadMultiply".into(),
                        '/' => "NumpadDivide".into(),
                        '.' => "NumpadDecimal".into(),
                        '=' => "NumpadEqual".into(),
                        _ => text,
                    }
                }
                _ => text,
            },
        };
        let mut parts = Vec::new();
        for (flag, name) in [
            (NSEventModifierFlags::Control, "ctrl"),
            (NSEventModifierFlags::Option, "alt"),
            (NSEventModifierFlags::Shift, "shift"),
            (NSEventModifierFlags::Command, "super"),
        ] {
            if event.modifierFlags().contains(flag) {
                parts.push(name.to_owned());
            }
        }
        parts.push(key);
        match HotKey::from_str(&parts.join("+")) {
            Ok(key) => {
                owner.save_shortcut(&key.into_string());
                self.finish();
            }
            Err(error) => owner.error(&error.to_string()),
        }
    }

    pub fn finish(&self) {
        if self.ivars().recording.replace(false)
            && let Some(owner) = self.ivars().owner.load()
        {
            owner.restore_shortcut();
        }
    }
}
