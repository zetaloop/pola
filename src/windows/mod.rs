use std::{
    cell::{Cell, RefCell},
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    rc::Rc,
};

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use jiff::Zoned;
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, WPARAM,
        },
        System::{
            Com::{CLSCTX_ALL, CoCreateInstance},
            Threading::CreateMutexW,
        },
        UI::{
            Shell::{DWPOS_SPAN, DesktopWallpaper, IDesktopWallpaper},
            WindowsAndMessaging::{
                FindWindowW, HWND_BROADCAST, KillTimer, MB_ICONERROR, MB_OK, MessageBoxW,
                PBT_APMRESUMEAUTOMATIC, PostMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
                SetTimer, WM_APP, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_THEMECHANGED,
                WM_TIMECHANGE, WM_TIMER,
            },
        },
    },
    core::{PCWSTR, w},
};
use windows_notifyicon::{NotifyIcon, NotifyIconEvent};
use windows_reactor::*;
use windows_registry::CURRENT_USER;
use windows_window::Window;

use crate::{
    config::{Config, Profile},
    mode::Mode,
    schedule::Event,
    shortcut::Shortcut,
};

mod settings;

const TIMER_ID: usize = 1;
const SHOW_SETTINGS: u32 = WM_APP + 1;
const RUNTIME: windows::core::PCWSTR = w!("io.github.zetaloop.pola.runtime");
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
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
    Open(Callback<()>),
}

pub(crate) struct AppState {
    _instance: Instance,
    app: AppContext,
    config: RefCell<Config>,
    applied: Cell<Option<Mode>>,
    next: RefCell<Option<Event>>,
    icon: RefCell<Option<NotifyIcon>>,
    message_window: RefCell<Option<Window>>,
    settings: RefCell<OpenWindow>,
    shortcut: RefCell<Shortcut>,
}

impl AppState {
    fn new(app: &AppContext, config: Config, instance: Instance) -> Rc<Self> {
        Rc::new(Self {
            _instance: instance,
            app: app.clone(),
            config: RefCell::new(config),
            applied: Cell::new(None),
            next: RefCell::new(None),
            icon: RefCell::new(None),
            message_window: RefCell::new(None),
            settings: RefCell::new(OpenWindow::Closed),
            shortcut: RefCell::new(Shortcut::default()),
        })
    }

    fn start(self: &Rc<Self>) -> windows_notifyicon::Result<()> {
        self.add_message_window()?;
        self.add_icon()?;

        let shortcut = self.config.borrow().general.shortcut.clone();
        if let Err(error) = self.register_hotkey(&shortcut) {
            show_error("Could not register shortcut", &error.to_string());
        }

        let mode = {
            let config = self.config.borrow();
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

        let address = Rc::as_ptr(self) as usize;
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed {
                return;
            }
            let state = unsafe { &*(address as *const AppState) };
            if state.shortcut.borrow().matches(event.id) {
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
                    SHOW_SETTINGS => {
                        state.open_settings();
                        Some(0)
                    }
                    WM_SETTINGCHANGE => {
                        state.appearance_changed();
                        state.schedule_next();
                        None
                    }
                    WM_TIMECHANGE => {
                        state.reconcile_schedule();
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
                    NotifyIconEvent::Activate { .. } => state.open_settings(),
                    NotifyIconEvent::ContextMenu { position } => state.show_menu(position),
                    NotifyIconEvent::Unavailable => state.exit(),
                    _ => {}
                }
            })
            .build()?;
        *self.icon.borrow_mut() = Some(icon);
        Ok(())
    }

    fn show_menu(self: &Rc<Self>, position: windows_notifyicon::Point) {
        let target = self.system_mode().toggle();
        let toggle = format!("Switch to {target}");
        let schedule = if self.config.borrow().schedule.enabled {
            "Disable schedule"
        } else {
            "Enable schedule"
        };
        let next = self
            .config
            .borrow()
            .schedule
            .next(&Zoned::now())
            .map(|event| format!("Next: {} → {}", event.at.strftime("%a %H:%M"), event.mode))
            .unwrap_or_else(|| "Next: —".into());

        let state = Rc::clone(self);
        let toggle_action = toggle.clone();
        let menu = Menu::new(
            [
                MenuItem::disabled("next", next),
                MenuItem::separator("separator-1"),
                MenuItem::item("toggle", toggle),
                MenuItem::item("schedule", schedule),
                MenuItem::separator("separator-2"),
                MenuItem::item("settings", "Settings…"),
                MenuItem::separator("separator-3"),
                MenuItem::item("exit", "Quit"),
            ],
            move |label: String| {
                if label == toggle_action {
                    state.toggle();
                } else if label == schedule {
                    state.toggle_schedule();
                } else {
                    match label.as_str() {
                        "Settings…" => state.open_settings(),
                        "Quit" => state.exit(),
                        _ => {}
                    }
                }
            },
        );

        if let Err(error) = self
            .app
            .show_menu_at(ScreenPoint::new(position.x, position.y), menu)
        {
            show_error("Could not show menu", &error.to_string());
        }
    }

    fn open_settings(self: &Rc<Self>) {
        let activate = {
            let mut window = self.settings.borrow_mut();
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
            _ = activate.call(());
            return;
        }

        if let Err(error) = self.app.open_window(View::component::<settings::Settings>(
            settings::SettingsInput(Rc::clone(self)),
        )) {
            *self.settings.borrow_mut() = OpenWindow::Closed;
            show_error("Could not open settings", &error.to_string());
        }
    }

    pub(crate) fn settings_opened(&self, activate: Callback<()>) {
        *self.settings.borrow_mut() = OpenWindow::Open(activate);
    }

    pub(crate) fn settings_closed(&self) {
        *self.settings.borrow_mut() = OpenWindow::Closed;
    }

    fn exit(&self) {
        if let Err(error) = self.app.exit() {
            show_error("Could not exit pola", &error.to_string());
        }
    }

    fn toggle(&self) {
        self.select(self.system_mode().toggle());
        self.schedule_next();
    }

    fn toggle_schedule(&self) {
        let mut config = self.config.borrow().clone();
        config.schedule.enabled = !config.schedule.enabled;

        if let Err(error) = config.save() {
            show_error("Could not save settings", &error.to_string());
            return;
        }

        *self.config.borrow_mut() = config;
        self.schedule_next();
    }

    pub(crate) fn config(&self) -> Config {
        self.config.borrow().clone()
    }

    pub(crate) fn launch_at_login(&self) -> bool {
        launch_at_login()
    }

    pub(crate) fn save_config(&self, config: Config, launch: bool) -> Result<(), String> {
        let old = self.config.borrow().clone();
        let old_launch = launch_at_login();

        self.register_hotkey(&config.general.shortcut)
            .map_err(|error| error.to_string())?;

        if old_launch != launch
            && let Err(error) = set_launch_at_login(launch)
        {
            _ = self.register_hotkey(&old.general.shortcut);
            return Err(error);
        }

        if let Err(error) = config.save() {
            _ = self.register_hotkey(&old.general.shortcut);
            if old_launch != launch {
                _ = set_launch_at_login(old_launch);
            }
            return Err(error.to_string());
        }

        let mode = self.system_mode();
        let profile_changed = old.profile(mode) != config.profile(mode);

        *self.config.borrow_mut() = config;
        if profile_changed {
            self.applied.set(None);
            self.apply(mode);
        }
        self.schedule_next();
        Ok(())
    }

    fn system_mode(&self) -> Mode {
        CURRENT_USER
            .open(PERSONALIZE)
            .and_then(|key| key.get_u32("SystemUsesLightTheme"))
            .map_or(Mode::Light, |value| {
                if value == 0 { Mode::Dark } else { Mode::Light }
            })
    }

    fn set_system_mode(&self, mode: Mode) -> Result<(), String> {
        let key = CURRENT_USER
            .create(PERSONALIZE)
            .map_err(|error| error.to_string())?;
        let value = u32::from(mode == Mode::Light);
        key.set_u32("SystemUsesLightTheme", value)
            .and_then(|_| key.set_u32("AppsUseLightTheme", value))
            .map_err(|error| error.to_string())?;

        let setting = "ImmersiveColorSet"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(setting.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                2000,
                None,
            );
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_THEMECHANGED,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                2000,
                None,
            );
        }
        Ok(())
    }

    fn select(&self, mode: Mode) {
        if self.system_mode() != mode
            && let Err(error) = self.set_system_mode(mode)
        {
            show_error("Could not change appearance", &error);
            return;
        }
        self.apply(mode);
    }

    fn appearance_changed(&self) {
        self.apply(self.system_mode());
    }

    fn apply(&self, mode: Mode) {
        if self.applied.get() == Some(mode) {
            return;
        }

        let profile = self.config.borrow().profile(mode).clone();
        self.apply_profile(&profile);
        self.applied.set(Some(mode));
    }

    fn apply_profile(&self, profile: &Profile) {
        let mut errors = Vec::new();

        if let Some(path) = &profile.wallpaper
            && let Err(error) = set_wallpaper(path)
        {
            errors.push(format!("Wallpaper: {error}"));
        }

        for command in &profile.commands {
            if let Err(error) = command.run() {
                errors.push(format!("{}: {error}", command.program));
            }
        }

        if !errors.is_empty() {
            show_error("Could not apply appearance", &errors.join("\n"));
        }
    }

    fn register_hotkey(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.shortcut.borrow_mut().register(text)
    }

    fn schedule_fired(&self) {
        if let Some(mode) = self.config.borrow().schedule.current(&Zoned::now()) {
            self.select(mode);
        }
        self.schedule_next();
    }

    fn schedule_next(&self) {
        let next = self.config.borrow().schedule.next(&Zoned::now());
        *self.next.borrow_mut() = next.clone();

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
            .next
            .borrow()
            .as_ref()
            .is_some_and(|event| event.at.timestamp() <= now.timestamp());

        if missed && let Some(mode) = self.config.borrow().schedule.current(&now) {
            self.select(mode);
        }
        self.schedule_next();
    }

    fn reconcile_schedule(&self) {
        let now = Zoned::now();
        if let Some(mode) = self.config.borrow().schedule.current(&now) {
            self.select(mode);
        }
        self.schedule_next();
    }
}

fn set_wallpaper(path: &std::path::Path) -> windows::core::Result<()> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    unsafe {
        let wallpaper: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)?;
        wallpaper.SetPosition(DWPOS_SPAN)?;
        wallpaper.SetWallpaper(PCWSTR::null(), PCWSTR(wide.as_ptr()))?;
    }
    Ok(())
}

fn icon_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("pola.ico")))
        .unwrap_or_else(|| PathBuf::from("pola.ico"))
}

fn launch_at_login() -> bool {
    CURRENT_USER.open(RUN).is_ok_and(|key| {
        key.values()
            .is_ok_and(|mut values| values.any(|(name, _)| name.eq_ignore_ascii_case("pola")))
    })
}

fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    if launch_at_login() == enabled {
        return Ok(());
    }

    let key = CURRENT_USER
        .create(RUN)
        .map_err(|error| error.to_string())?;

    if enabled {
        let path = std::env::current_exe().map_err(|error| error.to_string())?;
        key.set_string("pola", format!("\"{}\"", path.display()))
            .map_err(|error| error.to_string())
    } else {
        key.remove_value("pola").map_err(|error| error.to_string())
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
            PostMessageW(Some(window), SHOW_SETTINGS, WPARAM(0), LPARAM(0))?;
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
