use std::{os::windows::ffi::OsStrExt, path::Path};

use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM},
        System::Com::{CLSCTX_ALL, CoCreateInstance},
        UI::{
            Shell::{DWPOS_SPAN, DesktopWallpaper, IDesktopWallpaper},
            WindowsAndMessaging::{
                HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
                WM_THEMECHANGED,
            },
        },
    },
    core::PCWSTR,
};
use windows_registry::CURRENT_USER;

use crate::mode::Mode;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn mode() -> Result<Mode, String> {
    CURRENT_USER
        .open(PERSONALIZE)
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .map(|value| if value == 0 { Mode::Dark } else { Mode::Light })
        .map_err(|error| error.to_string())
}

pub fn set_mode(mode: Mode) -> Result<(), String> {
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

pub fn launch_at_login() -> bool {
    let Ok(path) = std::env::current_exe() else {
        return false;
    };
    let expected = format!("\"{}\" daemon", path.display());
    CURRENT_USER
        .open(RUN)
        .and_then(|key| key.get_string("pola"))
        .is_ok_and(|value| value == expected)
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let key = CURRENT_USER
        .create(RUN)
        .map_err(|error| error.to_string())?;
    if enabled {
        let path = std::env::current_exe().map_err(|error| error.to_string())?;
        key.set_string("pola", format!("\"{}\" daemon", path.display()))
            .map_err(|error| error.to_string())
    } else if key.get_string("pola").is_ok() {
        key.remove_value("pola").map_err(|error| error.to_string())
    } else {
        Ok(())
    }
}

pub fn set_wallpaper(path: &Path) -> Result<(), String> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
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
