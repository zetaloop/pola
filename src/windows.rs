use std::{
    iter,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0},
        System::{
            Com::{
                CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_QUERY_VALUE, REG_NOTIFY_CHANGE_LAST_SET,
                RRF_RT_REG_DWORD, RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue,
                RegOpenKeyExW,
            },
            Threading::{
                CREATE_NO_WINDOW, CreateEventW, CreateMutexW, INFINITE, WaitForSingleObject,
            },
        },
        UI::Shell::{DWPOS_SPAN, DesktopWallpaper, IDesktopWallpaper},
    },
    core::{PCWSTR, w},
};

use crate::{
    config::{Command as ConfigCommand, Config},
    mode::Mode,
    report,
};

const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            _ = CloseHandle(self.0);
        }
    }
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            _ = RegCloseKey(self.0);
        }
    }
}

struct Apartment;

impl Apartment {
    fn new() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
            .map_err(|error| error.to_string())?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn instance() -> Result<Option<Handle>, String> {
    let handle = unsafe { CreateMutexW(None, false, w!("Local\\io.github.zetaloop.pola")) }
        .map_err(|error| error.to_string())?;
    let handle = Handle(handle);
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        Ok(None)
    } else {
        Ok(Some(handle))
    }
}

fn open_personalize() -> Result<RegistryKey, String> {
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PERSONALIZE,
            None,
            KEY_NOTIFY | KEY_QUERY_VALUE,
            &mut key,
        )
    }
    .ok()
    .map_err(|error| error.to_string())?;
    Ok(RegistryKey(key))
}

fn mode(key: HKEY) -> Result<Mode, String> {
    let mut value = 0u32;
    let mut size = std::mem::size_of_val(&value) as u32;
    unsafe {
        RegGetValueW(
            key,
            PCWSTR::null(),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    }
    .ok()
    .map_err(|error| error.to_string())?;
    Ok(if value == 0 { Mode::Dark } else { Mode::Light })
}

fn watch(key: HKEY, event: HANDLE) -> Result<(), String> {
    unsafe { RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, Some(event), true) }
        .ok()
        .map_err(|error| error.to_string())
}

fn wait_for_change(event: HANDLE) -> Result<(), String> {
    if unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(windows::core::Error::from_thread().to_string())
    }
}

fn apply(config: &Config, mode: Mode, data: &Path, log: &Path) -> bool {
    let profile = config.profile(mode);
    let mut success = true;

    if let Some(wallpaper) = &profile.wallpaper {
        let path = if wallpaper.is_absolute() {
            wallpaper.clone()
        } else {
            data.join(wallpaper)
        };
        if let Err(error) = set_wallpaper(&path) {
            report(log, format!("Wallpaper: {error}"));
            success = false;
        }
    }

    for command in &profile.commands {
        if let Err(error) = run_command(command) {
            report(log, format!("{}: {error}", command.program.display()));
            success = false;
        }
    }

    success
}

fn set_wallpaper(path: &Path) -> Result<(), String> {
    let _apartment = Apartment::new()?;
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        let wallpaper: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)
            .map_err(|error| error.to_string())?;
        wallpaper
            .SetPosition(DWPOS_SPAN)
            .and_then(|_| wallpaper.SetWallpaper(PCWSTR::null(), PCWSTR(wide.as_ptr())))
            .map_err(|error| error.to_string())
    }
}

fn run_command(command: &ConfigCommand) -> Result<(), String> {
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0);
    process.spawn().map(drop).map_err(|error| error.to_string())
}

pub fn run(config: &Config, data: &Path, log: &Path) -> Result<(), String> {
    let Some(_instance) = instance()? else {
        return Ok(());
    };
    let key = open_personalize()?;
    let event = Handle(
        unsafe { CreateEventW(None, false, false, None) }.map_err(|error| error.to_string())?,
    );
    watch(key.0, event.0)?;
    let mut applied = None;
    let current = mode(key.0)?;
    if apply(config, current, data, log) {
        applied = Some(current);
    }

    loop {
        wait_for_change(event.0)?;
        watch(key.0, event.0)?;
        let current = mode(key.0)?;
        if applied != Some(current) && apply(config, current, data, log) {
            applied = Some(current);
        }
    }
}
