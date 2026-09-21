use std::{
    cell::OnceCell,
    rc::Rc,
    sync::{OnceLock, mpsc},
};

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use jiff::Zoned;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
    UI::WindowsAndMessaging::{
        KillTimer, PBT_APMRESUMEAUTOMATIC, PostMessageW, SetTimer, WM_APP, WM_POWERBROADCAST,
        WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMECHANGE, WM_TIMER,
    },
};
use windows_window::Window;

use crate::{config::Config, ipc, runtime::Runtime};

const INCOMING: u32 = WM_APP + 1;
const TIMER: usize = 1;
static CHANNEL: OnceLock<(mpsc::Sender<Message>, usize)> = OnceLock::new();

enum Message {
    Request(ipc::Incoming),
    Hotkey(u32),
}

struct Daemon {
    runtime: Runtime,
    window: OnceCell<Window>,
}

impl Daemon {
    fn schedule(&self) {
        let next = self.runtime.schedule();
        let hwnd = HWND(self.window.get().unwrap().hwnd());
        unsafe {
            _ = KillTimer(Some(hwnd), TIMER);
        }
        let Some(event) = next else { return };
        let now = Zoned::now().timestamp().as_nanosecond();
        let at = event.at.timestamp().as_nanosecond();
        let milliseconds = ((at - now).max(1) / 1_000_000).clamp(1, u32::MAX as i128) as u32;
        if unsafe { SetTimer(Some(hwnd), TIMER, milliseconds, None) } == 0 {
            self.runtime
                .report(windows::core::Error::from_thread().to_string());
        }
    }
}

pub fn receive(incoming: ipc::Incoming) {
    let (sender, address) = CHANNEL.get().expect("daemon channel is initialized");
    if sender.send(Message::Request(incoming)).is_ok()
        && let Err(error) =
            unsafe { PostMessageW(Some(HWND(*address as _)), INCOMING, WPARAM(0), LPARAM(0)) }
    {
        eprintln!("Daemon notification: {error}");
    }
}

pub fn run(ready: bool) -> Result<(), String> {
    let Some((_lock, listener)) = ipc::listen(ready).map_err(|error| error.to_string())? else {
        if ready {
            ipc::ready().map_err(|error| error.to_string())?;
        }
        return Ok(());
    };
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
        .map_err(|error| error.to_string())?;
    let daemon = Rc::new(Daemon {
        runtime: Runtime::new(Config::load().map_err(|error| error.to_string())?),
        window: OnceCell::new(),
    });
    let (sender, receiver) = mpsc::channel();
    let events = Rc::downgrade(&daemon);
    let window = Window::new("io.github.zetaloop.pola.daemon")
        .visible(false)
        .on_message(move |_, message, wparam, _| {
            let daemon = events.upgrade()?;
            let result = match message {
                INCOMING => {
                    for message in receiver.try_iter() {
                        match message {
                            Message::Request(request) => daemon.runtime.receive(request),
                            Message::Hotkey(id) => {
                                let matches = daemon.runtime.shortcut.borrow().matches(id);
                                if matches && let Err(error) = daemon.runtime.toggle() {
                                    daemon.runtime.report(error);
                                }
                            }
                        }
                    }
                    Ok(())
                }
                WM_SETTINGCHANGE | WM_THEMECHANGED => daemon.runtime.observe(),
                WM_TIMECHANGE => daemon.runtime.resume(),
                WM_POWERBROADCAST if wparam as u32 == PBT_APMRESUMEAUTOMATIC => {
                    daemon.runtime.resume()
                }
                WM_TIMER if wparam == TIMER => daemon.runtime.scheduled(),
                _ => return None,
            };
            if let Err(error) = result {
                daemon.runtime.report(error);
            }
            daemon.schedule();
            None
        })
        .create()
        .map_err(|error| error.to_string())?;
    let address = window.hwnd() as usize;
    daemon.window.set(window).ok().unwrap();
    let keys = sender.clone();
    CHANNEL.set((sender, address)).ok().unwrap();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed
            && keys.send(Message::Hotkey(event.id)).is_ok()
            && let Err(error) =
                unsafe { PostMessageW(Some(HWND(address as _)), INCOMING, WPARAM(0), LPARAM(0)) }
        {
            eprintln!("Shortcut notification: {error}");
        }
    }));
    daemon.runtime.start();
    daemon.schedule();
    ipc::serve(listener, receive);
    if ready {
        ipc::ready().map_err(|error| error.to_string())?;
    }
    windows_window::run();
    drop(daemon);
    unsafe {
        CoUninitialize();
    }
    Ok(())
}
