use std::path::Path;

use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSScreen, NSWorkspace,
};
use objc2_foundation::{NSAppleScript, NSArray, NSString, NSURL};

use crate::mode::Mode;

pub fn mode(mtm: MainThreadMarker) -> Mode {
    let app = NSApplication::sharedApplication(mtm);
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

pub fn set_mode(mode: Mode) -> Result<(), String> {
    let source = NSString::from_str(match mode {
        Mode::Light => {
            "tell application \"System Events\" to tell appearance preferences to set dark mode to false"
        }
        Mode::Dark => {
            "tell application \"System Events\" to tell appearance preferences to set dark mode to true"
        }
    });
    let script = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source)
        .ok_or("Could not create appearance script")?;
    let mut error = None;
    unsafe {
        script.executeAndReturnError(Some(&mut error));
    }
    match error {
        Some(error) => Err(format!("{error:?}")),
        None => Ok(()),
    }
}

pub fn set_wallpaper(path: &Path) -> Result<(), String> {
    let mtm = MainThreadMarker::new().expect("wallpaper changes require the main thread");
    let workspace = NSWorkspace::sharedWorkspace();
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let mut errors = Vec::new();
    for screen in NSScreen::screens(mtm).iter() {
        let options = workspace
            .desktopImageOptionsForScreen(&screen)
            .unwrap_or_default();
        if let Err(error) =
            unsafe { workspace.setDesktopImageURL_forScreen_options_error(&url, &screen, &options) }
        {
            errors.push(format!("{error:?}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}
