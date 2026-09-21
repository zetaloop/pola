use std::{
    cell::{Cell, OnceCell, RefCell},
    ffi::c_void,
    ptr,
};

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use jiff::Zoned;
use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions, NSNotification,
    NSNotificationCenter, NSObject, NSObjectNSKeyValueObserverRegistration, NSObjectProtocol,
    NSSize, NSString, NSSystemClockDidChangeNotification, NSSystemTimeZoneDidChangeNotification,
    NSTimer, ns_string,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::{
    config::{Config, Profile},
    mode::Mode,
    runtime::Runtime,
};

mod file;
mod profile;
mod schedule;
mod settings;
mod shortcut;
pub(crate) mod system;
mod ui;
mod window;

struct DelegateIvars {
    runtime: Runtime,
    timer: RefCell<Option<Retained<NSTimer>>>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    settings: OnceCell<Retained<settings::Settings>>,
    window: OnceCell<window::Window>,
    appearance_observed: Cell<bool>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.start();
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            self.open_window();
            true
        }
    }

    unsafe impl NSToolbarDelegate for Delegate {
        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn toolbar_items(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            toolbar_identifiers()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn toolbar_allowed_items(
            &self,
            _toolbar: &NSToolbar,
        ) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            toolbar_identifiers()
        }

        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_item(
            &self,
            _toolbar: &NSToolbar,
            identifier: &NSToolbarItemIdentifier,
            _insert: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            let item = match identifier.to_string().as_str() {
                "mode" => Some(("Appearance", "circle.lefthalf.filled", sel!(selectMode:))),
                "settings" => Some(("Settings", "gearshape", sel!(showSettings:))),
                _ => None,
            };
            item.map(|(title, symbol, action)| {
                let item =
                    NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(self.mtm()), identifier);
                item.setLabel(&NSString::from_str(title));
                item.setImage(Some(&ui::symbol(symbol, title)));
                item.setBordered(true);
                if identifier.to_string() == "mode" {
                    let control = unsafe {
                        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                            &NSArray::from_retained_slice(&[
                                NSString::from_str("Light"),
                                NSString::from_str("Dark"),
                            ]),
                            NSSegmentSwitchTracking::SelectOne,
                            Some(self),
                            Some(action),
                            self.mtm(),
                        )
                    };
                    control.setControlSize(NSControlSize::Large);
                    item.setView(Some(&control));
                } else {
                    unsafe {
                        item.setTarget(Some(self));
                        item.setAction(Some(action));
                    }
                }
                item
            })
        }
    }

    unsafe impl NSTableViewDataSource for Delegate {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn navigation_rows(&self, _table: &NSTableView) -> isize {
            2
        }
    }

    unsafe impl NSControlTextEditingDelegate for Delegate {}

    unsafe impl NSTableViewDelegate for Delegate {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn navigation_cell(
            &self,
            _table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            let (name, symbol) = if row == 0 {
                ("Appearance", "circle.lefthalf.filled")
            } else {
                ("Schedule", "calendar")
            };
            Some(ui::cell(self.mtm(), name, Some(&ui::symbol(symbol, name))).into_super())
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn navigation_changed(&self, notification: &NSNotification) {
            if let Some(object) = notification.object()
                && let Ok(table) = object.downcast::<NSTableView>()
                && table.selectedRow() >= 0
                && let Some(window) = self.ivars().window.get()
            {
                window.show_page(table.selectedRow());
            }
        }
    }

    impl Delegate {
        #[unsafe(method(showWindow:))]
        fn show_window(&self, _sender: &NSObject) {
            self.open_window();
        }

        #[unsafe(method(selectMode:))]
        fn select_mode(&self, sender: &NSSegmentedControl) {
            self.select(if sender.selectedSegment() == 0 {
                Mode::Light
            } else {
                Mode::Dark
            });
            self.update_window();
        }

        #[unsafe(method(editAppearance:))]
        fn edit_appearance(&self, sender: &NSButton) {
            let window = self.ivars().window.get().unwrap();
            window.window.makeFirstResponder(None);
            let mode = if sender.tag() == 0 {
                Mode::Light
            } else {
                Mode::Dark
            };
            window.inspect(mode);
        }

        #[unsafe(method(showSchedule:))]
        fn show_schedule(&self, _sender: &NSObject) {
            self.open_window();
            self.ivars().window.get().unwrap().show_page(1);
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: &NSObject) {
            self.open_settings();
        }

        #[unsafe(method(scheduleFired:))]
        fn schedule_fired(&self, _timer: &NSTimer) {
            let now = Zoned::now();
            if let Some(mode) = self.config().schedule.current(&now) {
                self.select(mode);
            }
            self.schedule_next();
        }

        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn appearance_changed(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            self.apply(self.system_mode());
            self.update_window();
        }

        #[unsafe(method(clockChanged:))]
        fn clock_changed(&self, _notification: &NSNotification) {
            self.resume_schedule();
        }

        #[unsafe(method(didWake:))]
        fn did_wake(&self, _notification: &NSNotification) {
            self.resume_schedule();
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &NSObject) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Drop for Delegate {
    fn drop(&mut self) {
        if self.ivars().appearance_observed.get() {
            unsafe {
                NSApplication::sharedApplication(self.mtm())
                    .removeObserver_forKeyPath(self, ns_string!("effectiveAppearance"));
            }
        }
    }
}

impl Delegate {
    fn new(mtm: objc2_foundation::MainThreadMarker, config: Config) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            runtime: Runtime::new(config),
            timer: RefCell::new(None),
            status_item: OnceCell::new(),
            settings: OnceCell::new(),
            window: OnceCell::new(),
            appearance_observed: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn start(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        self.build_menu();
        self.observe_system();

        let shortcut = self.config().shortcut;
        if let Err(error) = self.register_hotkey(&shortcut) {
            show_error(
                self.mtm(),
                "Could not register shortcut",
                &error.to_string(),
            );
        }

        let mode = {
            let config = self.config();
            if config.schedule.enabled && config.schedule.apply_on_launch {
                config.schedule.current(&Zoned::now())
            } else {
                None
            }
        };

        match mode {
            Some(mode) => self.select(mode),
            None => self.apply(self.system_mode()),
        }
        self.schedule_next();
        self.update_window();
    }

    fn observe_system(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        unsafe {
            app.addObserver_forKeyPath_options_context(
                self,
                ns_string!("effectiveAppearance"),
                NSKeyValueObservingOptions::New,
                ptr::null_mut(),
            );
        }
        self.ivars().appearance_observed.set(true);

        let center = NSNotificationCenter::defaultCenter();
        unsafe {
            center.addObserver_selector_name_object(
                self,
                sel!(clockChanged:),
                Some(NSSystemClockDidChangeNotification),
                None,
            );
            center.addObserver_selector_name_object(
                self,
                sel!(clockChanged:),
                Some(NSSystemTimeZoneDidChangeNotification),
                None,
            );
        }

        let workspace = NSWorkspace::sharedWorkspace();
        unsafe {
            workspace
                .notificationCenter()
                .addObserver_selector_name_object(
                    self,
                    sel!(didWake:),
                    Some(NSWorkspaceDidWakeNotification),
                    None,
                );
        }
    }

    fn build_menu(&self) {
        let mtm = self.mtm();
        let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(-2.0);
        if let Some(button) = status_item.button(mtm) {
            let data = NSData::with_bytes(include_bytes!("../../assets/pola-symbol.png"));
            let image =
                NSImage::initWithData(NSImage::alloc(), &data).expect("invalid status image");
            image.setSize(NSSize::new(18.0, 18.0));
            image.setTemplate(true);
            button.setImage(Some(&image));
            button.setToolTip(Some(ns_string!("pola")));
            unsafe {
                button.setTarget(Some(self));
                button.setAction(Some(sel!(showWindow:)));
            }
        }
        self.ivars().status_item.set(status_item).unwrap();

        let menu = NSMenu::new(mtm);
        let application = NSMenu::new(mtm);
        for (title, action, key) in [
            ("Show pola", sel!(showWindow:), "0"),
            ("Settings…", sel!(showSettings:), ","),
            ("Quit pola", sel!(quit:), "q"),
        ] {
            let item = unsafe {
                application.addItemWithTitle_action_keyEquivalent(
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::from_str(key),
                )
            };
            unsafe { item.setTarget(Some(self)) };
        }
        let root = NSMenuItem::new(mtm);
        root.setSubmenu(Some(&application));
        menu.addItem(&root);

        let edit = NSMenu::new(mtm);
        edit.setTitle(ns_string!("Edit"));
        for (title, action, key) in [
            ("Undo", sel!(undo:), "z"),
            ("Cut", sel!(cut:), "x"),
            ("Copy", sel!(copy:), "c"),
            ("Paste", sel!(paste:), "v"),
            ("Select All", sel!(selectAll:), "a"),
        ] {
            unsafe {
                edit.addItemWithTitle_action_keyEquivalent(
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::from_str(key),
                );
            }
        }
        let root = NSMenuItem::new(mtm);
        root.setSubmenu(Some(&edit));
        menu.addItem(&root);
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&menu));
    }

    fn open_window(&self) {
        let window = self
            .ivars()
            .window
            .get_or_init(|| window::Window::new(self.mtm(), self));
        window.show();
        self.update_window();
    }

    fn open_settings(&self) {
        let settings = self
            .ivars()
            .settings
            .get_or_init(|| settings::Settings::new(self.mtm(), self));
        settings.show();
    }

    fn update_window(&self) {
        if let Some(window) = self.ivars().window.get() {
            let next = self.ivars().runtime.next.borrow().clone();
            window.update(&self.config(), self.system_mode(), next.as_ref());
        }
        if let Some(settings) = self.ivars().settings.get() {
            settings.update();
        }
    }

    fn config(&self) -> Config {
        self.ivars().runtime.config.borrow().clone()
    }

    fn save_config(&self, config: Config) -> Result<(), String> {
        self.ivars().runtime.save(config)?;
        self.schedule_next();
        Ok(())
    }

    fn system_mode(&self) -> Mode {
        system::mode(self.mtm())
    }

    fn toggle(&self) {
        self.select(self.system_mode().toggle());
        self.schedule_next();
    }

    fn select(&self, mode: Mode) {
        if self.system_mode() != mode
            && let Err(error) = system::set_mode(mode)
        {
            show_error(self.mtm(), "Could not change appearance", &error);
            return;
        }
        self.apply(mode);
    }

    fn apply(&self, mode: Mode) {
        if let Err(error) = self.ivars().runtime.apply(mode) {
            show_error(self.mtm(), "Could not apply appearance", &error);
        }
    }

    fn save_profile(&self, mode: Mode, profile: Profile) -> Result<(), String> {
        let mut config = self.config();
        if config.profile(mode) == &profile {
            return Ok(());
        }
        match mode {
            Mode::Light => config.light = profile,
            Mode::Dark => config.dark = profile,
        }
        self.save_config(config)
    }

    fn register_hotkey(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.ivars().runtime.shortcut.borrow_mut().register(text)
    }

    fn schedule_next(&self) {
        let next = self.config().schedule.next(&Zoned::now());
        *self.ivars().runtime.next.borrow_mut() = next.clone();
        if let Some(timer) = self.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }

        let Some(event) = next else {
            self.update_window();
            return;
        };

        let now = Zoned::now().timestamp().as_nanosecond();
        let at = event.at.timestamp().as_nanosecond();
        let seconds = (at - now).max(1) as f64 / 1_000_000_000.0;
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                seconds,
                self,
                sel!(scheduleFired:),
                None,
                false,
            )
        };
        *self.ivars().timer.borrow_mut() = Some(timer);
        self.update_window();
    }

    fn resume_schedule(&self) {
        let now = Zoned::now();
        let missed = self
            .ivars()
            .runtime
            .next
            .borrow()
            .as_ref()
            .is_some_and(|event| event.at.timestamp() <= now.timestamp());

        if missed && let Some(mode) = self.config().schedule.current(&now) {
            self.select(mode);
        }
        self.schedule_next();
    }
}

fn toolbar_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    NSArray::from_slice(&[
        unsafe { NSToolbarToggleSidebarItemIdentifier },
        unsafe { NSToolbarFlexibleSpaceItemIdentifier },
        ns_string!("mode"),
        unsafe { NSToolbarToggleInspectorItemIdentifier },
        ns_string!("settings"),
    ])
}

pub(super) fn launch_at_login() -> bool {
    unsafe {
        matches!(
            SMAppService::mainAppService().status(),
            SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval
        )
    }
}

fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    if launch_at_login() == enabled {
        return Ok(());
    }

    unsafe {
        let service = SMAppService::mainAppService();
        let result = if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        };
        result.map_err(|error| format!("{error:?}"))
    }
}

fn show_error(mtm: objc2_foundation::MainThreadMarker, title: &str, message: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    alert.runModal();
}

pub fn run() {
    let mtm = objc2_foundation::MainThreadMarker::new().expect("pola must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            show_error(mtm, "Could not load pola", &error.to_string());
            return;
        }
    };
    let delegate = Delegate::new(mtm, config);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let address = &*delegate as *const Delegate as usize;
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed {
            return;
        }

        let delegate = unsafe { &*(address as *const Delegate) };
        let matches = delegate.ivars().runtime.shortcut.borrow().matches(event.id);
        if matches {
            delegate.toggle();
        }
    }));

    app.run();
}
