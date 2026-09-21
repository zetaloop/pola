use std::cell::{OnceCell, RefCell};

use crate::locale::tr;

use dispatch2::DispatchQueue;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{NSArray, NSNotification, NSObject, NSObjectProtocol, NSString};

use crate::{
    config::{Config, Profile},
    ipc::{Client, Request},
    mode::Mode,
};

mod action;
mod appearance;
pub(crate) mod daemon;
mod file;
pub(crate) mod locale;
mod profile;
mod profiles;
mod schedule;
mod settings;
mod shortcut;
pub(crate) mod system;
mod ui;
mod window;

thread_local! {
    static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
}

struct DelegateIvars {
    client: Client,
    window: OnceCell<window::Window>,
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
            self.build_menu();
            self.open_window();
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            self.open_window();
            true
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn terminating(&self, _notification: &NSNotification) {
            if let Some(window) = self.ivars().window.get() { window.settings.finish(); }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_close(&self, _app: &NSApplication) -> bool {
            true
        }
    }

    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowDidResignKey:))]
        fn resign_key(&self, _notification: &NSNotification) {
            if let Some(window) = self.ivars().window.get() { window.settings.finish(); }
        }

        #[unsafe(method(windowWillClose:))]
        fn closing(&self, _notification: &NSNotification) {
            if let Some(window) = self.ivars().window.get() { window.settings.finish(); }
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
    }

    unsafe impl NSTableViewDataSource for Delegate {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn navigation_rows(&self, _table: &NSTableView) -> isize {
            4
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
            let (name, symbol) = [
                (tr!("Appearance"), "circle.lefthalf.filled"),
                (tr!("Configurations"), "list.bullet"),
                (tr!("Schedule"), "calendar"),
                (tr!("Settings"), "gearshape"),
            ][row as usize];
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

        #[unsafe(method(showProfiles:))]
        fn profiles(&self, _sender: &NSObject) {
            self.show_profiles();
        }

        #[unsafe(method(dismissError:))]
        fn dismiss_error(&self, _sender: &NSObject) {
            if let Some(window) = self.ivars().window.get() { window.error(""); }
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: &NSObject) {
            self.open_settings();
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &NSObject) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Delegate {
    fn new(mtm: objc2_foundation::MainThreadMarker, client: Client) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            client,
            window: OnceCell::new(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn build_menu(&self) {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let application = NSMenu::new(mtm);
        for (title, action, key) in [
            (tr!("Show pola"), sel!(showWindow:), "0"),
            (tr!("Settings…"), sel!(showSettings:), ","),
            (tr!("Quit pola"), sel!(quit:), "q"),
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
        edit.setTitle(&NSString::from_str(tr!("Edit")));
        for (title, action, key) in [
            (tr!("Undo"), sel!(undo:), "z"),
            (tr!("Cut"), sel!(cut:), "x"),
            (tr!("Copy"), sel!(copy:), "c"),
            (tr!("Paste"), sel!(paste:), "v"),
            (tr!("Select All"), sel!(selectAll:), "a"),
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
        self.open_window();
        if let Some(window) = self.ivars().window.get() {
            window.show_page(3);
        }
    }

    fn update_window(&self) {
        if let Some(window) = self.ivars().window.get() {
            let state = self.ivars().client.state();
            window.update(
                state.mode.expect("AppKit appearance is available"),
                state.next.as_ref(),
            );
        }
    }

    fn config(&self) -> Config {
        self.ivars().client.state().config
    }

    fn save_config(&self, config: Config) -> Result<(), String> {
        let language_changed = self.config().language != config.language;
        self.ivars().client.request(Request::Save(config))?;
        if language_changed {
            locale::apply();
        }
        self.update_window();
        Ok(())
    }

    fn select(&self, mode: Mode) {
        let error = self
            .ivars()
            .client
            .request(Request::Select(mode))
            .err()
            .unwrap_or_default();
        if let Some(window) = self.ivars().window.get() {
            window.error(&error);
        }
        self.update_window();
    }

    fn save_profile(&self, name: Option<&str>, profile: Profile) -> Result<(), String> {
        let mut config = self.config();
        if let Some(name) = name {
            let existing = config
                .profiles
                .iter_mut()
                .find(|profile| profile.name == name)
                .ok_or(tr!("This configuration has been removed."))?;
            if existing == &profile {
                return Ok(());
            }
            *existing = profile;
        } else {
            config.profiles.push(profile);
        }
        self.save_config(config)
    }

    fn run_profile(&self, name: &str) -> Result<(), String> {
        self.ivars().client.request(Request::Run {
            name: name.into(),
            wait: false,
        })
    }

    fn show_profiles(&self) {
        if let Some(window) = self.ivars().window.get() {
            window.profiles.show_list();
            window.show_page(1);
        }
    }

    fn register_hotkey(&self, text: &str) -> Result<(), String> {
        self.ivars().client.request(Request::Shortcut(text.into()))
    }

    fn launch_at_login(&self) -> bool {
        self.ivars().client.state().launch_at_login
    }

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        self.ivars().client.request(Request::Launch(enabled))
    }
}

fn toolbar_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    NSArray::from_slice(&[
        unsafe { NSToolbarToggleSidebarItemIdentifier },
        unsafe { NSToolbarSidebarTrackingSeparatorItemIdentifier },
        unsafe { NSToolbarFlexibleSpaceItemIdentifier },
    ])
}

pub(crate) fn show_error(title: &str, message: &str) {
    let mtm = objc2_foundation::MainThreadMarker::new().expect("alerts require the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    alert.runModal();
}

pub fn run() -> Result<(), String> {
    let mtm = objc2_foundation::MainThreadMarker::new().expect("pola must run on the main thread");
    let client = Client::connect(|error| {
        DispatchQueue::main().exec_async(move || {
            if let Some(delegate) = DELEGATE.with(|slot| slot.borrow().clone()) {
                delegate.update_window();
                if let Some(error) = error {
                    if let Some(window) = delegate.ivars().window.get() {
                        window.error(&error);
                    } else {
                        show_error("pola", &error);
                    }
                }
            }
        });
    })?;
    locale::apply();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = Delegate::new(mtm, client);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
    app.run();
    DELEGATE.with(|slot| slot.borrow_mut().take());
    Ok(())
}
