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
    NSAppleScript, NSArray, NSData, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions,
    NSNotification, NSNotificationCenter, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSObjectProtocol, NSSize, NSString, NSSystemClockDidChangeNotification,
    NSSystemTimeZoneDidChangeNotification, NSTimer, NSURL, ns_string,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::{
    config::{Config, Profile},
    mode::Mode,
    schedule::Event,
    shortcut::Shortcut,
};

mod file;
mod profile;
mod schedule;
mod settings;
mod shortcut;
mod ui;
mod window;

struct State {
    config: Config,
    applied: Option<Mode>,
    timer: Option<Retained<NSTimer>>,
    next: Option<Event>,
    shortcut: Shortcut,
}

impl State {
    fn new(config: Config) -> Self {
        Self {
            config,
            applied: None,
            timer: None,
            next: None,
            shortcut: Shortcut::default(),
        }
    }
}

struct DelegateIvars {
    state: RefCell<State>,
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
            state: RefCell::new(State::new(config)),
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

        let shortcut = self.ivars().state.borrow().config.shortcut.clone();
        if let Err(error) = self.register_hotkey(&shortcut) {
            show_error(
                self.mtm(),
                "Could not register shortcut",
                &error.to_string(),
            );
        }

        let mode = {
            let state = self.ivars().state.borrow();
            if state.config.schedule.enabled && state.config.schedule.apply_on_launch {
                state.config.schedule.current(&Zoned::now())
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
            let state = self.ivars().state.borrow();
            window.update(&state.config, self.system_mode(), state.next.as_ref());
        }
        if let Some(settings) = self.ivars().settings.get() {
            settings.update();
        }
    }

    fn config(&self) -> Config {
        self.ivars().state.borrow().config.clone()
    }

    fn save_config(&self, config: Config) -> Result<(), String> {
        let old = self.config();
        self.register_hotkey(&config.shortcut)
            .map_err(|error| error.to_string())?;
        if let Err(error) = config.save() {
            if let Err(restore) = self.register_hotkey(&old.shortcut) {
                return Err(format!("{error}\nShortcut: {restore}"));
            }
            return Err(error.to_string());
        }
        let mode = self.system_mode();
        let changed = old.profile(mode) != config.profile(mode);
        self.ivars().state.borrow_mut().config = config;
        if changed {
            self.ivars().state.borrow_mut().applied = None;
            self.apply(mode);
        }
        self.schedule_next();
        Ok(())
    }

    fn system_mode(&self) -> Mode {
        let app = NSApplication::sharedApplication(self.mtm());
        let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        let names = NSArray::from_slice(&[aqua, dark_aqua]);
        match app
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&names)
            .as_deref()
        {
            Some(name) if name == dark_aqua => Mode::Dark,
            _ => Mode::Light,
        }
    }

    fn toggle(&self) {
        self.select(self.system_mode().toggle());
        self.schedule_next();
    }

    fn select(&self, mode: Mode) {
        if self.system_mode() != mode {
            let source = NSString::from_str(match mode {
                Mode::Light => {
                    "tell application \"System Events\" to tell appearance preferences to set dark mode to false"
                }
                Mode::Dark => {
                    "tell application \"System Events\" to tell appearance preferences to set dark mode to true"
                }
            });
            if let Some(script) = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source) {
                let mut error = None;
                unsafe {
                    script.executeAndReturnError(Some(&mut error));
                }
                if let Some(error) = error {
                    show_error(
                        self.mtm(),
                        "Could not change appearance",
                        &format!("{error:?}"),
                    );
                    return;
                }
            }
        }

        self.apply(mode);
    }

    fn apply(&self, mode: Mode) {
        {
            let state = self.ivars().state.borrow();
            if state.applied == Some(mode) {
                return;
            }
        }

        let profile = self.ivars().state.borrow().config.profile(mode).clone();
        self.apply_profile(&profile);
        self.ivars().state.borrow_mut().applied = Some(mode);
    }

    fn apply_profile(&self, profile: &Profile) {
        let mut errors = Vec::new();

        if let Some(path) = &profile.wallpaper {
            let workspace = NSWorkspace::sharedWorkspace();
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));

            for screen in NSScreen::screens(self.mtm()).iter() {
                let options = workspace
                    .desktopImageOptionsForScreen(&screen)
                    .unwrap_or_default();
                if let Err(error) = unsafe {
                    workspace.setDesktopImageURL_forScreen_options_error(&url, &screen, &options)
                } {
                    errors.push(format!("Wallpaper: {error:?}"));
                }
            }
        }

        for command in &profile.commands {
            if let Err(error) = command.run() {
                errors.push(format!("{}: {error}", command.program));
            }
        }

        if !errors.is_empty() {
            show_error(self.mtm(), "Could not apply appearance", &errors.join("\n"));
        }
    }

    fn save_profile(&self, mode: Mode, profile: Profile) -> Result<(), String> {
        let mut config = self.ivars().state.borrow().config.clone();
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
        self.ivars().state.borrow_mut().shortcut.register(text)
    }

    fn schedule_next(&self) {
        let next = self
            .ivars()
            .state
            .borrow()
            .config
            .schedule
            .next(&Zoned::now());

        let mut state = self.ivars().state.borrow_mut();
        if let Some(timer) = state.timer.take() {
            timer.invalidate();
        }
        state.next = next.clone();

        let Some(event) = next else {
            drop(state);
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
        state.timer = Some(timer);
        drop(state);
        self.update_window();
    }

    fn resume_schedule(&self) {
        let now = Zoned::now();
        let missed = self
            .ivars()
            .state
            .borrow()
            .next
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
        let matches = delegate.ivars().state.borrow().shortcut.matches(event.id);
        if matches {
            delegate.toggle();
        }
    }));

    app.run();
}
