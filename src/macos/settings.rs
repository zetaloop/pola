use std::cell::OnceCell;

use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::{
    NSColor, NSControlStateValueOn, NSGridCellPlacement, NSSwitch, NSTextField, NSWindow,
    NSWindowDelegate,
};
use objc2_foundation::{MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSString};

use super::{Delegate, shortcut::Recorder, ui};

pub struct Ivars {
    owner: Weak<Delegate>,
    window: Retained<NSWindow>,
    launch: Retained<NSSwitch>,
    apply: Retained<NSSwitch>,
    recorder: OnceCell<Retained<Recorder>>,
    error: Retained<NSTextField>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Settings;

    unsafe impl NSObjectProtocol for Settings {}
    unsafe impl NSWindowDelegate for Settings {
        #[unsafe(method(windowWillClose:))]
        fn closing(&self, _notification: &NSNotification) {
            self.ivars().recorder.get().unwrap().finish();
        }
    }

    impl Settings {
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

        #[unsafe(method(applyChanged:))]
        fn apply_changed(&self, sender: &NSSwitch) {
            if let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                config.schedule.apply_on_launch = sender.state() == NSControlStateValueOn;
                match owner.save_config(config) {
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
        let window = ui::window(mtm, "Settings", 480.0, 260.0);
        let launch = NSSwitch::new(mtm);
        let apply = NSSwitch::new(mtm);
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            window,
            launch,
            apply,
            recorder: OnceCell::new(),
            error,
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.ivars()
            .window
            .setDelegate(Some(ProtocolObject::from_ref(&*this)));
        for (control, action) in [
            (&this.ivars().launch, sel!(launchChanged:)),
            (&this.ivars().apply, sel!(applyChanged:)),
        ] {
            unsafe {
                control.setTarget(Some(&this));
                control.setAction(Some(action));
            }
        }
        let launch_label = ui::label(mtm, "Launch at login");
        let apply_label = ui::label(mtm, "Apply schedule on launch");
        let recorder = this
            .ivars()
            .recorder
            .get_or_init(|| Recorder::new(mtm, &this));
        let clear = ui::button(mtm, "Clear", &this, sel!(clearShortcut:));
        let shortcut_label = ui::label(mtm, "Global shortcut");
        let controls = ui::stack(mtm, true, &[recorder, &clear]);
        let form = ui::form(
            mtm,
            &[
                [&launch_label, &this.ivars().launch],
                [&apply_label, &this.ivars().apply],
                [&shortcut_label, &controls],
            ],
        );
        form.columnAtIndex(1)
            .setXPlacement(NSGridCellPlacement::Trailing);
        let content = ui::stack(mtm, false, &[&form, &this.ivars().error]);
        form.widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        this.ivars()
            .error
            .widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        ui::mount(&this.ivars().window.contentView().unwrap(), &content, 24.0);
        this.update();
        this
    }

    pub fn show(&self) {
        self.update();
        ui::show(&self.ivars().window);
    }

    pub fn update(&self) {
        if let Some(owner) = self.ivars().owner.load() {
            let config = owner.config();
            self.ivars()
                .launch
                .setState(isize::from(owner.launch_at_login()));
            self.ivars()
                .apply
                .setState(isize::from(config.schedule.apply_on_launch));
            self.ivars()
                .recorder
                .get()
                .unwrap()
                .display(&config.shortcut);
        }
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
