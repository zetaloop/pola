use std::cell::{OnceCell, RefCell};

use dispatch2::DispatchQueue;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{NSArray, NSNotification, NSObject, NSObjectProtocol, NSString, ns_string};

use crate::{
    config::{Config, Profile},
    ipc::{Client, Request},
    mode::Mode,
};

pub(crate) mod daemon;
mod file;
mod profile;
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
    settings: OnceCell<Retained<settings::Settings>>,
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

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_close(&self, _app: &NSApplication) -> bool {
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
            settings: OnceCell::new(),
            window: OnceCell::new(),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn build_menu(&self) {
        let mtm = self.mtm();
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
            let state = self.ivars().client.state();
            window.update(
                &state.config,
                state.mode.expect("AppKit appearance is available"),
                state.next.as_ref(),
            );
        }
        if let Some(settings) = self.ivars().settings.get() {
            settings.update();
        }
    }

    fn config(&self) -> Config {
        self.ivars().client.state().config
    }

    fn save_config(&self, config: Config) -> Result<(), String> {
        self.ivars().client.request(Request::Save(config))?;
        self.update_window();
        Ok(())
    }

    fn system_mode(&self) -> Mode {
        self.ivars()
            .client
            .state()
            .mode
            .expect("AppKit appearance is available")
    }

    fn select(&self, mode: Mode) {
        if let Err(error) = self.ivars().client.request(Request::Select(mode)) {
            show_error("Could not change appearance", &error);
        }
        self.update_window();
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
        unsafe { NSToolbarFlexibleSpaceItemIdentifier },
        ns_string!("mode"),
        unsafe { NSToolbarToggleInspectorItemIdentifier },
        ns_string!("settings"),
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
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let client = Client::connect(|error| {
        DispatchQueue::main().exec_async(move || {
            if let Some(delegate) = DELEGATE.with(|slot| slot.borrow().clone()) {
                delegate.update_window();
                if let Some(error) = error {
                    show_error("pola", &error);
                }
            }
        });
    })?;
    let delegate = Delegate::new(mtm, client);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
    app.run();
    DELEGATE.with(|slot| slot.borrow_mut().take());
    Ok(())
}
