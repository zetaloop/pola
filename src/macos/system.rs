use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use objc2::{AnyThread, MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSScreen, NSWorkspace,
};
use objc2_foundation::{
    NSAppleScript, NSArray, NSData, NSDictionary, NSNumber, NSObject, NSPropertyListFormat,
    NSPropertyListMutabilityOptions, NSPropertyListSerialization, NSString, NSURL, ns_string,
};

use crate::{locale::tr, mode::Mode};

pub fn mode() -> Result<Mode, String> {
    let mtm = MainThreadMarker::new().expect("appearance reads require the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let names = NSArray::from_slice(&[aqua, dark_aqua]);
    Ok(
        match app
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&names)
            .as_deref()
        {
            Some(name) if name == dark_aqua => Mode::Dark,
            _ => Mode::Light,
        },
    )
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
        .ok_or(tr!("Could not create appearance script"))?;
    let mut error = None;
    unsafe {
        script.executeAndReturnError(Some(&mut error));
    }
    match error {
        Some(error) => Err(format!("{error:?}")),
        None => Ok(()),
    }
}

fn agent_path() -> Result<PathBuf, String> {
    let home =
        std::env::var_os("HOME").ok_or_else(|| tr!("{variable} is not set", variable = "HOME"))?;
    Ok(PathBuf::from(home).join("Library/LaunchAgents/io.github.zetaloop.pola.plist"))
}

fn read_agent(path: &Path) -> Result<Option<Retained<NSDictionary>>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let plist = unsafe {
        NSPropertyListSerialization::propertyListWithData_options_format_error(
            &NSData::with_bytes(&bytes),
            NSPropertyListMutabilityOptions::Immutable,
            std::ptr::null_mut(),
        )
    }
    .map_err(|error| error.to_string())?;
    let agent = plist
        .downcast::<NSDictionary>()
        .map_err(|_| tr!("Login item must contain a property list dictionary"))?;
    if !agent
        .objectForKey(ns_string!("Label"))
        .and_then(|value| value.downcast::<NSString>().ok())
        .is_some_and(|value| &*value == ns_string!("io.github.zetaloop.pola"))
    {
        return Err(tr!("Login item path is occupied by another service").into());
    }
    Ok(Some(agent))
}

pub fn launch_at_login() -> bool {
    let Ok(path) = agent_path() else { return false };
    let Ok(Some(agent)) = read_agent(&path) else {
        return false;
    };
    let Some(args) = agent
        .objectForKey(ns_string!("ProgramArguments"))
        .and_then(|value| value.downcast::<NSArray<AnyObject>>().ok())
    else {
        return false;
    };
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    args.len() == 2
        && args
            .objectAtIndex(0)
            .downcast::<NSString>()
            .is_ok_and(|value| value.to_string() == executable.to_string_lossy())
        && args
            .objectAtIndex(1)
            .downcast::<NSString>()
            .is_ok_and(|value| &*value == ns_string!("daemon"))
        && agent
            .objectForKey(ns_string!("RunAtLoad"))
            .and_then(|value| value.downcast::<NSNumber>().ok())
            .is_some_and(|value| value.boolValue())
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let path = agent_path()?;
    let existing = read_agent(&path)?;
    if !enabled {
        if existing.is_some() {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable = NSString::from_str(&executable.to_string_lossy());
    let arguments = NSArray::from_slice(&[&*executable, ns_string!("daemon")]);
    let agent = NSDictionary::<NSString, NSObject>::from_slices(
        &[
            ns_string!("Label"),
            ns_string!("ProgramArguments"),
            ns_string!("RunAtLoad"),
        ],
        &[
            ns_string!("io.github.zetaloop.pola"),
            &arguments,
            &NSNumber::numberWithBool(true),
        ],
    );
    let data = unsafe {
        NSPropertyListSerialization::dataWithPropertyList_format_options_error(
            &agent,
            NSPropertyListFormat::XMLFormat_v1_0,
            0,
        )
    }
    .map_err(|error| error.to_string())?;
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(&data.to_vec())
        .map_err(|error| error.to_string())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(())
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
