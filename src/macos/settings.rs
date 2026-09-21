use std::cell::OnceCell;

use crate::locale::{Locale, tr};

use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    sel,
};
use objc2_app_kit::{
    NSAccessibility, NSColor, NSControlStateValueOn, NSGridCellPlacement, NSPopUpButton, NSSwitch,
    NSTextField, NSView,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSRect, NSString};

use super::{Delegate, shortcut::Recorder, ui};

pub struct Ivars {
    owner: Weak<Delegate>,
    view: Retained<NSView>,
    launch: Retained<NSSwitch>,
    language: Retained<NSPopUpButton>,
    recorder: OnceCell<Retained<Recorder>>,
    error: Retained<NSTextField>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Settings;

    unsafe impl NSObjectProtocol for Settings {}

    impl Settings {
        #[unsafe(method(languageChanged:))]
        fn language_changed(&self, sender: &NSPopUpButton) {
            if let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                config.language = match sender.indexOfSelectedItem() {
                    1 => Some(Locale::English), 2 => Some(Locale::Chinese), _ => None,
                };
                match owner.save_config(config) {
                    Ok(()) => self.error(""),
                    Err(error) => self.error(&error),
                }
                self.update();
            }
        }

        #[unsafe(method(launchChanged:))]
        fn launch_changed(&self, sender: &NSSwitch) {
            if let Some(owner) = self.ivars().owner.load() {
                match owner.set_launch_at_login(sender.state() == NSControlStateValueOn) {
                    Ok(()) => self.error(""),
                    Err(error) => self.error(&error),
                }
                self.update();
            }
        }

        #[unsafe(method(clearShortcut:))]
        fn clear_shortcut(&self, _sender: &NSObject) {
            self.ivars().recorder.get().unwrap().finish();
            self.save_shortcut("");
        }
    }
);

impl Settings {
    pub fn new(mtm: MainThreadMarker, owner: &Delegate) -> Retained<Self> {
        let view = NSView::new(mtm);
        let language =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
        language.addItemsWithTitles(&NSArray::from_retained_slice(&[
            NSString::from_str(tr!("System default")),
            NSString::from_str("English"),
            NSString::from_str("简体中文"),
        ]));
        let launch = NSSwitch::new(mtm);
        launch.setAccessibilityLabel(Some(&NSString::from_str(tr!("Launch at login"))));
        language.setAccessibilityLabel(Some(&NSString::from_str(tr!("Language"))));
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            view,
            launch,
            language,
            recorder: OnceCell::new(),
            error,
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe {
            this.ivars().launch.setTarget(Some(&this));
            this.ivars().launch.setAction(Some(sel!(launchChanged:)));
            this.ivars().language.setTarget(Some(&this));
            this.ivars()
                .language
                .setAction(Some(sel!(languageChanged:)));
        }
        let language_label = ui::label(mtm, tr!("Language"));
        let launch_label = ui::label(mtm, tr!("Launch at login"));
        let recorder = this
            .ivars()
            .recorder
            .get_or_init(|| Recorder::new(mtm, &this));
        let clear = ui::button(mtm, tr!("Clear"), &this, sel!(clearShortcut:));
        let shortcut_label = ui::label(mtm, tr!("Global shortcut"));
        let controls = ui::stack(mtm, true, &[recorder, &clear]);
        let form = ui::form(
            mtm,
            &[
                [&language_label, &this.ivars().language],
                [&launch_label, &this.ivars().launch],
                [&shortcut_label, &controls],
            ],
        );
        form.columnAtIndex(1)
            .setXPlacement(NSGridCellPlacement::Trailing);
        let title = ui::heading(mtm, tr!("Settings"));
        let content = ui::stack(mtm, false, &[&title, &form, &this.ivars().error]);
        form.widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        this.ivars()
            .error
            .widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        ui::scroll(&this.ivars().view, &content);
        this.update();
        this
    }

    pub fn view(&self) -> &NSView {
        &self.ivars().view
    }

    pub fn update(&self) {
        if let Some(owner) = self.ivars().owner.load() {
            let config = owner.config();
            self.ivars()
                .language
                .selectItemAtIndex(match config.language {
                    None => 0,
                    Some(Locale::English) => 1,
                    Some(Locale::Chinese) => 2,
                });
            self.ivars()
                .launch
                .setState(isize::from(owner.launch_at_login()));
            self.ivars()
                .recorder
                .get()
                .unwrap()
                .display(&config.shortcut);
        }
    }

    pub fn finish(&self) {
        self.ivars().recorder.get().unwrap().finish();
    }

    pub fn error(&self, error: &str) {
        self.ivars()
            .error
            .setStringValue(&NSString::from_str(error));
    }

    pub fn suspend_shortcut(&self) -> bool {
        let Some(owner) = self.ivars().owner.load() else {
            return false;
        };
        match owner.register_hotkey("") {
            Ok(()) => {
                self.error("");
                true
            }
            Err(error) => {
                self.error(&error.to_string());
                false
            }
        }
    }

    pub fn restore_shortcut(&self) {
        if let Some(owner) = self.ivars().owner.load() {
            if let Err(error) = owner.register_hotkey(&owner.config().shortcut) {
                self.error(&error.to_string());
            }
            self.update();
        }
    }

    pub fn save_shortcut(&self, shortcut: &str) -> bool {
        let Some(owner) = self.ivars().owner.load() else {
            return false;
        };
        let mut config = owner.config();
        config.shortcut = shortcut.into();
        match owner.save_config(config) {
            Ok(()) => {
                self.error("");
                self.update();
                true
            }
            Err(error) => {
                self.error(&error);
                false
            }
        }
    }
}
