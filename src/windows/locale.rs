use jiff::civil::Time;
use windows::{
    Win32::{Foundation::SYSTEMTIME, Globalization::*},
    core::{Error, PCWSTR, PWSTR},
};

#[allow(non_snake_case)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/language.rs"));
}

pub fn apply() -> Result<(), String> {
    bindings::ApplicationLanguages::SetPrimaryLanguageOverride(crate::locale::current().tag())
        .map_err(|error| error.to_string())
}

pub fn languages() -> Result<Vec<String>, String> {
    let mut count = 0;
    let mut size = 0;
    unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut size) }
        .map_err(|error| error.to_string())?;
    let mut buffer = vec![0; size as usize];
    unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut size,
        )
    }
    .map_err(|error| error.to_string())?;
    Ok(buffer
        .split(|value| *value == 0)
        .filter(|value| !value.is_empty())
        .map(String::from_utf16_lossy)
        .collect())
}

pub fn weekdays() -> Result<Vec<String>, String> {
    let locale: Vec<_> = crate::locale::current()
        .tag()
        .encode_utf16()
        .chain([0])
        .collect();
    (0..7)
        .map(|index| {
            format(|buffer| unsafe {
                GetLocaleInfoEx(
                    PCWSTR(locale.as_ptr()),
                    LOCALE_SABBREVDAYNAME1 + index,
                    buffer,
                )
            })
        })
        .collect()
}

pub fn time(time: Time) -> Result<String, String> {
    let time = SYSTEMTIME {
        wHour: time.hour() as u16,
        wMinute: time.minute() as u16,
        ..Default::default()
    };
    format(|buffer| unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            Some(&time),
            PCWSTR::null(),
            buffer,
        )
    })
}

pub fn clock() -> Result<&'static str, String> {
    let value = format(|buffer| unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_ITIME, buffer) })?;
    Ok(if value == "1" {
        "24HourClock"
    } else {
        "12HourClock"
    })
}

fn format(mut write: impl FnMut(Option<&mut [u16]>) -> i32) -> Result<String, String> {
    let size = write(None);
    if size == 0 {
        return Err(Error::from_thread().to_string());
    }
    let mut buffer = vec![0; size as usize];
    let length = write(Some(&mut buffer));
    if length == 0 {
        return Err(Error::from_thread().to_string());
    }
    Ok(String::from_utf16_lossy(&buffer[..length as usize - 1]))
}
