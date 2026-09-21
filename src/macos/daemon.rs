use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};

use dispatch2::DispatchQueue;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use interprocess::local_socket::Listener;
use jiff::Zoned;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSWorkspace,
    NSWorkspaceDidWakeNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions,
    NSNotification, NSNotificationCenter, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSObjectProtocol, NSString, NSSystemClockDidChangeNotification,
    NSSystemTimeZoneDidChangeNotification, NSTimer, ns_string,
};

use crate::{config::Config, ipc, runtime::Runtime};

thread_local! {
    static DAEMON: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
}

struct Ivars {
    runtime: Runtime,
    timer: RefCell<Option<Retained<NSTimer>>>,
    listener: RefCell<Option<Listener>>,
    observed: Cell<bool>,
    ready: bool,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self, _notification: &NSNotification) {
            if let Err(error) = self.start() {
                if self.ivars().ready {
                    _ = ipc::write(&mut std::io::stdout(), &Err::<(), _>(&error));
                }
                eprintln!("{error}");
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
        }
    }

    impl Delegate {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn appearance_changed(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            if let Err(error) = self.ivars().runtime.observe() {
                self.ivars().runtime.report(error);
            }
            self.schedule();
        }

        #[unsafe(method(scheduleFired:))]
        fn scheduled(&self, _timer: &NSTimer) {
            if let Err(error) = self.ivars().runtime.scheduled() {
                self.ivars().runtime.report(error);
            }
            self.schedule();
        }

        #[unsafe(method(clockChanged:))]
        fn clock_changed(&self, _notification: &NSNotification) {
            if let Err(error) = self.ivars().runtime.resume() {
                self.ivars().runtime.report(error);
            }
            self.schedule();
        }
    }
);

impl Delegate {
    fn new(
        mtm: MainThreadMarker,
        config: Config,
        listener: Listener,
        ready: bool,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            runtime: Runtime::new(config),
            timer: RefCell::new(None),
            listener: RefCell::new(Some(listener)),
            observed: Cell::new(false),
            ready,
        });
        unsafe { msg_send![super(this), init] }
    }

    fn start(&self) -> Result<(), String> {
        let app = NSApplication::sharedApplication(self.mtm());
        unsafe {
            app.addObserver_forKeyPath_options_context(
                self,
                ns_string!("effectiveAppearance"),
                NSKeyValueObservingOptions::New,
                ptr::null_mut(),
            );
        }
        self.ivars().observed.set(true);
        let center = NSNotificationCenter::defaultCenter();
        unsafe {
            for notification in [
                NSSystemClockDidChangeNotification,
                NSSystemTimeZoneDidChangeNotification,
            ] {
                center.addObserver_selector_name_object(
                    self,
                    sel!(clockChanged:),
                    Some(notification),
                    None,
                );
            }
            NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .addObserver_selector_name_object(
                    self,
                    sel!(clockChanged:),
                    Some(NSWorkspaceDidWakeNotification),
                    None,
                );
        }
        GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                DispatchQueue::main().exec_async(move || {
                    if let Some(daemon) = DAEMON.with(|slot| slot.borrow().clone()) {
                        let matches = daemon.ivars().runtime.shortcut.borrow().matches(event.id);
                        if matches {
                            if let Err(error) = daemon.ivars().runtime.toggle() {
                                daemon.ivars().runtime.report(error);
                            }
                            daemon.schedule();
                        }
                    }
                });
            }
        }));
        self.ivars().runtime.start();
        self.schedule();
        ipc::serve(self.ivars().listener.borrow_mut().take().unwrap(), receive);
        if self.ivars().ready {
            ipc::ready().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn schedule(&self) {
        let next = self.ivars().runtime.schedule();
        if let Some(timer) = self.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }
        let Some(event) = next else { return };
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
    }
}

impl Drop for Delegate {
    fn drop(&mut self) {
        if let Some(timer) = self.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }
        if self.ivars().observed.get() {
            unsafe {
                NSApplication::sharedApplication(self.mtm())
                    .removeObserver_forKeyPath(self, ns_string!("effectiveAppearance"));
            }
        }
    }
}

pub fn receive(incoming: ipc::Incoming) {
    DispatchQueue::main().exec_async(move || {
        if let Some(daemon) = DAEMON.with(|slot| slot.borrow().clone()) {
            daemon.ivars().runtime.receive(incoming);
            daemon.schedule();
        }
    });
}

pub fn run(ready: bool) -> Result<(), String> {
    let Some((_lock, listener)) = ipc::listen(ready).map_err(|error| error.to_string())? else {
        if ready {
            ipc::ready().map_err(|error| error.to_string())?;
        }
        return Ok(());
    };
    let mtm = MainThreadMarker::new().expect("daemon must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let delegate = Delegate::new(
        mtm,
        Config::load().map_err(|error| error.to_string())?,
        listener,
        ready,
    );
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DAEMON.with(|slot| *slot.borrow_mut() = Some(delegate));
    app.run();
    DAEMON.with(|slot| slot.borrow_mut().take());
    Ok(())
}
