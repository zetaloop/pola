use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, WPARAM},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{
            FindWindowW, MB_ICONERROR, MB_OK, MessageBoxW, PostMessageW, WM_APP,
        },
    },
    core::{PCWSTR, w},
};
use windows_reactor::*;
use windows_window::Window;

use crate::{
    config::{Config, Profile},
    ipc::{Client, Request},
    locale::tr,
    mode::Mode,
};

mod action;
mod appearance;
pub(crate) mod daemon;
pub(crate) mod locale;
mod profile;
mod schedule;
mod settings;
pub(crate) mod system;
mod window;

const SHOW_WINDOW: u32 = WM_APP + 1;
const FRONTEND: PCWSTR = w!("io.github.zetaloop.pola.frontend");

thread_local! {
    static APP: RefCell<Option<Rc<AppState>>> = const { RefCell::new(None) };
}

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
    client: Client,
    message_window: OnceCell<Window>,
    window: RefCell<OpenWindow>,
}

impl AppState {
    fn new(app: &AppContext, client: Client, instance: Instance) -> Rc<Self> {
        Rc::new(Self {
            _instance: instance,
            app: app.clone(),
            client,
            message_window: OnceCell::new(),
            window: RefCell::new(OpenWindow::Closed),
        })
    }

    fn start(self: &Rc<Self>) -> windows_window::Result<()> {
        locale::apply().map_err(std::io::Error::other)?;
        let state = Rc::downgrade(self);
        let window = Window::new("io.github.zetaloop.pola.frontend")
            .visible(false)
            .quit_on_close(false)
            .on_message(move |_, message, _, _| {
                if message == SHOW_WINDOW {
                    if let Some(state) = state.upgrade() {
                        state.open_window();
                    }
                    Some(0)
                } else {
                    None
                }
            })
            .create()?;
        self.message_window.set(window).ok().unwrap();
        self.open_window();
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
            show_error(tr!("Could not open window"), &error.to_string());
            self.exit();
        }
    }

    pub(crate) fn window_opened(&self, activate: Callback<window::Event>) {
        *self.window.borrow_mut() = OpenWindow::Open(activate);
    }

    pub(crate) fn window_closed(&self) {
        if !matches!(self.window.replace(OpenWindow::Closed), OpenWindow::Closed) {
            self.exit();
        }
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
        *self.window.borrow_mut() = OpenWindow::Closed;
        if let Err(error) = self.app.exit() {
            show_error(tr!("Could not exit pola"), &error.to_string());
        }
    }

    pub(crate) fn config(&self) -> Config {
        self.client.state().config
    }

    pub(crate) fn launch_at_login(&self) -> bool {
        self.client.state().launch_at_login
    }

    pub(crate) fn save_config(&self, config: Config, launch: bool) -> Result<(), String> {
        let language_changed = self.config().language != config.language;
        let old_launch = self.launch_at_login();
        if old_launch != launch {
            self.client.request(Request::Launch(launch))?;
        }
        if let Err(error) = self.client.request(Request::Save(config)) {
            if old_launch != launch
                && let Err(restore) = self.client.request(Request::Launch(old_launch))
            {
                return Err(tr!(
                    "{error}\nLaunch at login: {restore}",
                    error = error,
                    restore = restore
                ));
            }
            return Err(error);
        }
        if language_changed {
            locale::apply()?;
        }
        self.notify_window();
        Ok(())
    }

    fn save_profile(&self, name: Option<&str>, profile: Profile) -> Result<(), String> {
        let mut config = self.config();
        if let Some(name) = name {
            let current = config
                .profiles
                .iter_mut()
                .find(|profile| profile.name == name)
                .ok_or(tr!("This configuration has been removed."))?;
            *current = profile;
        } else {
            config.profiles.push(profile);
        }
        self.save_config(config, self.launch_at_login())
    }

    fn system_mode(&self) -> Result<Mode, String> {
        self.client.state().mode
    }

    fn select(&self, mode: Mode) {
        if let Err(error) = self.client.request(Request::Select(mode)) {
            show_error(tr!("Could not change appearance"), &error);
        }
        self.notify_window();
    }

    fn run_profile(&self, name: &str) -> Result<(), String> {
        self.client.request(Request::Run {
            name: name.into(),
            wait: false,
        })
    }

    fn register_hotkey(&self, text: &str) -> Result<(), String> {
        self.client.request(Request::Shortcut(text.into()))
    }
}

pub(crate) fn show_error(title: &str, message: &str) {
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
    let instance = Instance(unsafe {
        CreateMutexW(None, false, w!("Local\\io.github.zetaloop.pola.frontend"))?
    });
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let window = unsafe { FindWindowW(None, FRONTEND)? };
        unsafe {
            PostMessageW(Some(window), SHOW_WINDOW, WPARAM(0), LPARAM(0))?;
        }
        Ok(None)
    } else {
        Ok(Some(instance))
    }
}

pub fn run() -> Result<(), String> {
    let Some(instance) = instance().map_err(|error| error.to_string())? else {
        return Ok(());
    };
    let result = App::run_with(move |app| {
        let proxy = app.proxy();
        let client = Client::connect(move |error| {
            if let Err(dispatch_error) = proxy.dispatch(move |_| {
                if let Some(state) = APP.with(|slot| slot.borrow().clone()) {
                    state.notify_window();
                    if let Some(error) = error {
                        show_error("pola", &error);
                    }
                }
            }) {
                eprintln!("Window notification: {dispatch_error}");
            }
        })
        .map_err(std::io::Error::other)?;
        let state = AppState::new(app, client, instance);
        APP.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&state)));
        state.start()?;
        Ok(state)
    });
    APP.with(|slot| slot.borrow_mut().take());
    result.map_err(|error| error.to_string())
}
