use std::{
    cell::{OnceCell, RefCell},
    str::FromStr,
};

use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use jiff::Zoned;
use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject, sel,
};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy,
    NSApplicationDelegate, NSButton, NSMenu, NSMenuItem, NSScreen, NSStatusBar, NSStatusItem,
    NSWorkspace, NSWorkspaceDidWakeNotification,
};
use objc2_foundation::{
    NSAppleScript, NSArray, NSDistributedNotificationCenter, NSNotification, NSNotificationCenter,
    NSObject, NSObjectProtocol, NSString, NSSystemClockDidChangeNotification,
    NSSystemTimeZoneDidChangeNotification, NSTimer, NSURL,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::{
    config::{Config, Profile},
    mode::Mode,
    schedule::Event,
};

mod settings;

struct State {
    config: Config,
    applied: Option<Mode>,
    timer: Option<Retained<NSTimer>>,
    next: Option<Event>,
    hotkey_manager: Option<GlobalHotKeyManager>,
    hotkey: Option<HotKey>,
}

impl State {
    fn new() -> Self {
        Self {
            config: Config::load().unwrap_or_default(),
            applied: None,
            timer: None,
            next: None,
            hotkey_manager: None,
            hotkey: None,
        }
    }
}

struct DelegateIvars {
    state: RefCell<State>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    settings: OnceCell<settings::Settings>,
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
    }

    impl Delegate {
        #[unsafe(method(toggleMode:))]
        fn toggle_mode(&self, _sender: &NSObject) {
            self.toggle();
        }

        #[unsafe(method(toggleSchedule:))]
        fn toggle_schedule(&self, _sender: &NSObject) {
            {
                let mut state = self.ivars().state.borrow_mut();
                state.config.schedule.enabled = !state.config.schedule.enabled;
                if let Err(error) = state.config.save() {
                    eprintln!("failed to save config: {error}");
                }
            }
            self.schedule_next();
            self.update_menu();
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: &NSObject) {
            let settings = self
                .ivars()
                .settings
                .get_or_init(|| settings::Settings::new(self.mtm(), self));
            if !settings.is_visible() {
                settings.load(&self.ivars().state.borrow().config, self);
            }
            settings.show();
        }

        #[unsafe(method(addSchedule:))]
        fn add_schedule(&self, _sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.add_rule(self, None);
            }
        }

        #[unsafe(method(removeSchedule:))]
        fn remove_schedule(&self, sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.remove_rule(sender.tag() as usize);
            }
        }

        #[unsafe(method(browseLight:))]
        fn browse_light(&self, _sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.browse(Mode::Light);
            }
        }

        #[unsafe(method(browseDark:))]
        fn browse_dark(&self, _sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.browse(Mode::Dark);
            }
        }

        #[unsafe(method(addLightCommand:))]
        fn add_light_command(&self, _sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.add_command(self, Mode::Light, None);
            }
        }

        #[unsafe(method(addDarkCommand:))]
        fn add_dark_command(&self, _sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.add_command(self, Mode::Dark, None);
            }
        }

        #[unsafe(method(removeLightCommand:))]
        fn remove_light_command(&self, sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.remove_command(Mode::Light, sender.tag() as usize);
            }
        }

        #[unsafe(method(removeDarkCommand:))]
        fn remove_dark_command(&self, sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.remove_command(Mode::Dark, sender.tag() as usize);
            }
        }

        #[unsafe(method(addLightArgument:))]
        fn add_light_argument(&self, sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.add_argument(self, Mode::Light, sender.tag() as usize);
            }
        }

        #[unsafe(method(addDarkArgument:))]
        fn add_dark_argument(&self, sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                settings.add_argument(self, Mode::Dark, sender.tag() as usize);
            }
        }

        #[unsafe(method(removeLightArgument:))]
        fn remove_light_argument(&self, sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                let (command, argument) = settings::unpack(sender.tag());
                settings.remove_argument(Mode::Light, command, argument);
            }
        }

        #[unsafe(method(removeDarkArgument:))]
        fn remove_dark_argument(&self, sender: &NSButton) {
            if let Some(settings) = self.ivars().settings.get() {
                let (command, argument) = settings::unpack(sender.tag());
                settings.remove_argument(Mode::Dark, command, argument);
            }
        }

        #[unsafe(method(saveSettings:))]
        fn save_settings(&self, _sender: &NSObject) {
            let Some(settings) = self.ivars().settings.get() else {
                return;
            };
            let Ok(config) = settings.config() else {
                return;
            };
            let old = self.ivars().state.borrow().config.clone();
            let old_launch = launch_at_login();
            let launch = settings.launch_at_login();

            if let Err(error) = self.register_hotkey(&config.general.shortcut) {
                settings.show_error(&error.to_string());
                return;
            }

            if old_launch != launch
                && let Err(error) = set_launch_at_login(launch)
            {
                _ = self.register_hotkey(&old.general.shortcut);
                settings.show_error(&error);
                return;
            }

            if let Err(error) = config.save() {
                _ = self.register_hotkey(&old.general.shortcut);
                if old_launch != launch {
                    _ = set_launch_at_login(old_launch);
                }
                settings.show_error(&error.to_string());
                return;
            }

            self.ivars().state.borrow_mut().config = config;
            self.ivars().state.borrow_mut().applied = None;
            self.apply(self.system_mode());
            self.schedule_next();
            self.update_menu();
        }

        #[unsafe(method(scheduleFired:))]
        fn schedule_fired(&self, _timer: &NSTimer) {
            let now = Zoned::now();
            if let Some(mode) = self.ivars().state.borrow().config.schedule.current(&now) {
                self.select(mode);
            }
            self.schedule_next();
        }

        #[unsafe(method(appearanceChanged:))]
        fn appearance_changed(&self, _notification: &NSNotification) {
            self.apply(self.system_mode());
            self.update_menu();
        }

        #[unsafe(method(clockChanged:))]
        fn clock_changed(&self, _notification: &NSNotification) {
            self.reconcile_schedule();
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

impl Delegate {
    fn new(mtm: objc2_foundation::MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            state: RefCell::new(State::new()),
            status_item: OnceCell::new(),
            settings: OnceCell::new(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn start(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        self.build_menu();
        self.observe_system();

        let shortcut = self.ivars().state.borrow().config.general.shortcut.clone();
        if let Err(error) = self.register_hotkey(&shortcut) {
            eprintln!("failed to register hotkey: {error}");
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
        self.update_menu();
    }

    fn observe_system(&self) {
        let appearance = NSDistributedNotificationCenter::defaultCenter();
        let appearance_name = NSString::from_str("AppleInterfaceThemeChangedNotification");
        unsafe {
            appearance.addObserver_selector_name_object(
                self,
                sel!(appearanceChanged:),
                Some(&appearance_name),
                None,
            );
        }

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
        let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        if let Some(button) = status_item.button(mtm) {
            button.setTitle(&NSString::from_str("pola"));
        }

        let menu = NSMenu::new(mtm);
        let mode = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str("Light"),
                None,
                &NSString::new(),
            )
        };
        mode.setEnabled(false);

        let next = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str("Next: —"),
                None,
                &NSString::new(),
            )
        };
        next.setEnabled(false);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let toggle = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str("Toggle"),
                Some(sel!(toggleMode:)),
                &NSString::new(),
            )
        };
        unsafe { toggle.setTarget(Some(self)) };

        let schedule = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str("Schedule"),
                Some(sel!(toggleSchedule:)),
                &NSString::new(),
            )
        };
        unsafe { schedule.setTarget(Some(self)) };

        let settings = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str("Settings…"),
                Some(sel!(showSettings:)),
                &NSString::from_str(","),
            )
        };
        unsafe { settings.setTarget(Some(self)) };

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let quit = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str("Quit"),
                Some(sel!(quit:)),
                &NSString::from_str("q"),
            )
        };
        unsafe { quit.setTarget(Some(self)) };

        status_item.setMenu(Some(&menu));
        self.ivars().status_item.set(status_item).unwrap();
    }

    fn update_menu(&self) {
        let Some(status_item) = self.ivars().status_item.get() else {
            return;
        };
        let Some(menu) = status_item.menu(self.mtm()) else {
            return;
        };
        let items = menu.itemArray();

        if items.len() >= 5 {
            items
                .objectAtIndex(0)
                .setTitle(&NSString::from_str(match self.system_mode() {
                    Mode::Light => "Light",
                    Mode::Dark => "Dark",
                }));

            let next = self
                .ivars()
                .state
                .borrow()
                .config
                .schedule
                .next(&Zoned::now())
                .map(|event| format!("Next: {} → {:?}", event.at.strftime("%a %H:%M"), event.mode))
                .unwrap_or_else(|| "Next: —".into());
            items.objectAtIndex(1).setTitle(&NSString::from_str(&next));

            items.objectAtIndex(4).setState(
                if self.ivars().state.borrow().config.schedule.enabled {
                    objc2_app_kit::NSControlStateValueOn
                } else {
                    objc2_app_kit::NSControlStateValueOff
                },
            );
        }
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
                    eprintln!("failed to change appearance: {error:?}");
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
                    eprintln!("failed to set wallpaper: {error:?}");
                }
            }
        }

        for command in &profile.commands {
            if let Err(error) = command.run() {
                eprintln!("failed to run {}: {error}", command.program);
            }
        }
    }

    fn register_hotkey(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        let hotkey = HotKey::from_str(text)?;
        let mut state = self.ivars().state.borrow_mut();

        if state.hotkey == Some(hotkey) {
            return Ok(());
        }

        let manager = match state.hotkey_manager.take() {
            Some(manager) => manager,
            None => GlobalHotKeyManager::new()?,
        };

        let old = state.hotkey.take();
        if let Some(old) = old {
            manager.unregister(old)?;
        }

        if let Err(error) = manager.register(hotkey) {
            if let Some(old) = old {
                let _ = manager.register(old);
            }
            state.hotkey = old;
            state.hotkey_manager = Some(manager);
            return Err(error.into());
        }

        state.hotkey = Some(hotkey);
        state.hotkey_manager = Some(manager);
        Ok(())
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
            self.update_menu();
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
        self.update_menu();
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

        if missed && let Some(mode) = self.ivars().state.borrow().config.schedule.current(&now) {
            self.select(mode);
        }
        self.schedule_next();
    }

    fn reconcile_schedule(&self) {
        let now = Zoned::now();
        if let Some(mode) = self.ivars().state.borrow().config.schedule.current(&now) {
            self.select(mode);
        }
        self.schedule_next();
    }
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

pub fn run() {
    let mtm = objc2_foundation::MainThreadMarker::new().expect("pola must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let address = &*delegate as *const Delegate as usize;
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed {
            return;
        }

        let delegate = unsafe { &*(address as *const Delegate) };
        let matches = delegate
            .ivars()
            .state
            .borrow()
            .hotkey
            .is_some_and(|hotkey| hotkey.id() == event.id);
        if matches {
            delegate.toggle();
        }
    }));

    app.run();
}
