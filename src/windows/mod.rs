use std::{cell::RefCell, path::PathBuf, rc::Rc};

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use jiff::Zoned;
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, WPARAM,
        },
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{
            FindWindowW, KillTimer, MB_ICONERROR, MB_OK, MessageBoxW, PBT_APMRESUMEAUTOMATIC,
            PostMessageW, SetTimer, WM_APP, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_THEMECHANGED,
            WM_TIMECHANGE, WM_TIMER,
        },
    },
    core::{PCWSTR, w},
};
use windows_notifyicon::{NotifyIcon, NotifyIconEvent};
use windows_reactor::*;
use windows_registry::CURRENT_USER;
use windows_window::Window;

use crate::{config::Config, mode::Mode, runtime::Runtime};

mod profile;
mod schedule;
mod settings;
pub(crate) mod system;
mod window;

const TIMER_ID: usize = 1;
const SHOW_WINDOW: u32 = WM_APP + 1;
const RUNTIME: windows::core::PCWSTR = w!("io.github.zetaloop.pola.runtime");
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

struct Instance(HANDLE);

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            _ = CloseHandle(self.0);
        }
    }
}

enum OpenWindow {
    Closed,
    Opening,
    Open(Callback<window::Event>),
}

pub(crate) struct AppState {
    _instance: Instance,
    app: AppContext,
    runtime: Runtime,
    icon: RefCell<Option<NotifyIcon>>,
    message_window: RefCell<Option<Window>>,
    window: RefCell<OpenWindow>,
}

impl AppState {
    fn new(app: &AppContext, config: Config, instance: Instance) -> Rc<Self> {
        Rc::new(Self {
            _instance: instance,
            app: app.clone(),
            runtime: Runtime::new(config),
            icon: RefCell::new(None),
            message_window: RefCell::new(None),
            window: RefCell::new(OpenWindow::Closed),
        })
    }

    fn start(self: &Rc<Self>) -> windows_notifyicon::Result<()> {
        self.add_message_window()?;
        self.add_icon()?;

        let shortcut = self.config().shortcut;
        if let Err(error) = self.register_hotkey(&shortcut) {
            show_error("Could not register shortcut", &error.to_string());
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
            None => match self.system_mode() {
                Ok(mode) => self.apply(mode),
                Err(error) => show_error("Could not read appearance", &error),
            },
        }
        self.schedule_next();

        let address = Rc::as_ptr(self) as usize;
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed {
                return;
            }
            let state = unsafe { &*(address as *const AppState) };
            if state.runtime.shortcut.borrow().matches(event.id) {
                state.toggle();
            }
        }));

        Ok(())
    }

    fn add_message_window(self: &Rc<Self>) -> windows_notifyicon::Result<()> {
        let state = Rc::downgrade(self);
        let window = Window::new("io.github.zetaloop.pola.runtime")
            .visible(false)
            .quit_on_close(false)
            .on_message(move |_hwnd, message, wparam, _lparam| {
                let state = state.upgrade()?;
                match message {
                    SHOW_WINDOW => {
                        state.open_window();
                        Some(0)
                    }
                    WM_SETTINGCHANGE => {
                        state.appearance_changed();
                        state.schedule_next();
                        None
                    }
                    WM_THEMECHANGED => {
                        state.appearance_changed();
                        None
                    }
                    WM_TIMECHANGE => {
                        state.resume_schedule();
                        None
                    }
                    WM_POWERBROADCAST if wparam as u32 == PBT_APMRESUMEAUTOMATIC => {
                        state.resume_schedule();
                        Some(1)
                    }
                    WM_TIMER if wparam == TIMER_ID => {
                        state.schedule_fired();
                        Some(0)
                    }
                    _ => None,
                }
            })
            .create()?;
        *self.message_window.borrow_mut() = Some(window);
        Ok(())
    }

    fn add_icon(self: &Rc<Self>) -> windows_notifyicon::Result<()> {
        let events = Rc::downgrade(self);
        let icon = NotifyIcon::new(icon_path())
            .tooltip("pola")
            .on_event(move |event| {
                let Some(state) = events.upgrade() else {
                    return;
                };
                match event {
                    NotifyIconEvent::Activate { .. } => state.open_window(),
                    NotifyIconEvent::Unavailable => {
                        show_error("Could not restore notification icon", "pola will exit.");
                        state.exit();
                    }
                    _ => {}
                }
            })
            .build()?;
        *self.icon.borrow_mut() = Some(icon);
        Ok(())
    }

    fn open_window(self: &Rc<Self>) {
        let activate = {
            let mut window = self.window.borrow_mut();
            match &*window {
                OpenWindow::Closed => {
                    *window = OpenWindow::Opening;
                    None
                }
                OpenWindow::Opening => return,
                OpenWindow::Open(activate) => Some(activate.clone()),
            }
        };

        if let Some(activate) = activate {
            _ = activate.call(window::Event::Activate);
            return;
        }

        if let Err(error) =
            self.app
                .open_window(View::component::<window::Main>(window::WindowInput(
                    Rc::clone(self),
                )))
        {
            *self.window.borrow_mut() = OpenWindow::Closed;
            show_error("Could not open window", &error.to_string());
        }
    }

    pub(crate) fn window_opened(&self, activate: Callback<window::Event>) {
        *self.window.borrow_mut() = OpenWindow::Open(activate);
    }

    pub(crate) fn window_closed(&self) {
        *self.window.borrow_mut() = OpenWindow::Closed;
    }

    fn notify_window(&self) {
        let callback = match &*self.window.borrow() {
            OpenWindow::Open(callback) => Some(callback.clone()),
            _ => None,
        };
        if let Some(callback) = callback {
            _ = callback.call(window::Event::Changed);
        }
    }

    fn exit(&self) {
        if let Err(error) = self.app.exit() {
            show_error("Could not exit pola", &error.to_string());
        }
    }

    fn toggle(&self) {
        match self.system_mode() {
            Ok(mode) => self.select(mode.toggle()),
            Err(error) => show_error("Could not read appearance", &error),
        }
        self.schedule_next();
    }

    pub(crate) fn config(&self) -> Config {
        self.runtime.config.borrow().clone()
    }

    pub(crate) fn launch_at_login(&self) -> bool {
        launch_at_login()
    }

    pub(crate) fn save_config(&self, config: Config, launch: bool) -> Result<(), String> {
        let old_launch = launch_at_login();
        if old_launch != launch {
            set_launch_at_login(launch)?;
        }
        if let Err(error) = self.runtime.save(config) {
            if old_launch != launch
                && let Err(restore) = set_launch_at_login(old_launch)
            {
                return Err(format!("{error}\nLaunch at login: {restore}"));
            }
            return Err(error);
        }
        self.schedule_next();
        Ok(())
    }

    fn system_mode(&self) -> Result<Mode, String> {
        system::mode()
    }

    fn select(&self, mode: Mode) {
        if !self.system_mode().is_ok_and(|current| current == mode)
            && let Err(error) = system::set_mode(mode)
        {
            show_error("Could not change appearance", &error);
            return;
        }
        self.apply(mode);
    }

    fn appearance_changed(&self) {
        match self.system_mode() {
            Ok(mode) => self.apply(mode),
            Err(error) => show_error("Could not read appearance", &error),
        }
    }

    fn apply(&self, mode: Mode) {
        if let Err(error) = self.runtime.apply(mode) {
            show_error("Could not apply appearance", &error);
        }
        self.notify_window();
    }

    fn register_hotkey(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime.shortcut.borrow_mut().register(text)
    }

    fn schedule_fired(&self) {
        if let Some(mode) = self.config().schedule.current(&Zoned::now()) {
            self.select(mode);
        }
        self.schedule_next();
    }

    fn schedule_next(&self) {
        let next = self.config().schedule.next(&Zoned::now());
        *self.runtime.next.borrow_mut() = next.clone();
        self.notify_window();

        let window = self.message_window.borrow();
        let Some(window) = window.as_ref() else {
            return;
        };
        let hwnd = HWND(window.hwnd());

        unsafe {
            _ = KillTimer(Some(hwnd), TIMER_ID);
        }

        let Some(event) = next else {
            return;
        };

        let now = Zoned::now().timestamp().as_nanosecond();
        let at = event.at.timestamp().as_nanosecond();
        let milliseconds = ((at - now).max(1) / 1_000_000).clamp(1, u32::MAX as i128) as u32;

        let timer = unsafe { SetTimer(Some(hwnd), TIMER_ID, milliseconds, None) };
        if timer == 0 {
            show_error(
                "Could not schedule appearance change",
                &windows::core::Error::from_thread().to_string(),
            );
        }
    }

    fn resume_schedule(&self) {
        let now = Zoned::now();
        let missed = self
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

fn icon_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("pola.ico")))
        .unwrap_or_else(|| PathBuf::from("pola.ico"))
}

fn launch_at_login() -> bool {
    let Ok(path) = std::env::current_exe() else {
        return false;
    };
    let expected = format!("\"{}\"", path.display());

    CURRENT_USER
        .open(RUN)
        .and_then(|key| key.get_string("pola"))
        .is_ok_and(|value| value == expected)
}

fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let key = CURRENT_USER
        .create(RUN)
        .map_err(|error| error.to_string())?;

    if enabled {
        let path = std::env::current_exe().map_err(|error| error.to_string())?;
        key.set_string("pola", format!("\"{}\"", path.display()))
            .map_err(|error| error.to_string())
    } else if key.get_string("pola").is_ok() {
        key.remove_value("pola").map_err(|error| error.to_string())
    } else {
        Ok(())
    }
}

fn show_error(title: &str, message: &str) {
    let title = title
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let message = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    unsafe {
        _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn instance() -> windows::core::Result<Option<Instance>> {
    let instance =
        Instance(unsafe { CreateMutexW(None, false, w!("Local\\io.github.zetaloop.pola"))? });

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let window = unsafe { FindWindowW(None, RUNTIME)? };
        unsafe {
            PostMessageW(Some(window), SHOW_WINDOW, WPARAM(0), LPARAM(0))?;
        }
        Ok(None)
    } else {
        Ok(Some(instance))
    }
}

pub fn run() {
    let instance = match instance() {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            show_error("Could not start pola", &error.to_string());
            return;
        }
    };

    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            show_error("Could not load pola", &error.to_string());
            return;
        }
    };

    if let Err(error) = App::run_with(move |app| {
        let state = AppState::new(app, config, instance);
        state.start()?;
        Ok(state)
    }) {
        show_error("Could not start pola", &error.to_string());
    }
}
