use jiff::civil::Time;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{
    NSArgumentDomain, NSArray, NSDate, NSDateFormatter, NSDateFormatterStyle, NSLocale,
    NSMutableCopying, NSString, NSTimeZone, NSUserDefaults, ns_string,
};

pub fn apply() {
    let defaults = NSUserDefaults::standardUserDefaults();
    let domain = defaults
        .volatileDomainForName(unsafe { NSArgumentDomain })
        .mutableCopy();
    let languages =
        NSArray::from_retained_slice(&[NSString::from_str(crate::locale::current().tag())]);
    unsafe {
        domain.setObject_forKey(
            &languages,
            ProtocolObject::from_ref(ns_string!("AppleLanguages")),
        );
        defaults.setVolatileDomain_forName(&domain, NSArgumentDomain);
    }
}

pub fn languages() -> Result<Vec<String>, String> {
    Ok(NSLocale::preferredLanguages()
        .iter()
        .map(|language| language.to_string())
        .collect())
}

pub fn weekdays() -> Result<Vec<String>, String> {
    let formatter = NSDateFormatter::new();
    formatter.setLocale(Some(&NSLocale::localeWithLocaleIdentifier(
        &NSString::from_str(crate::locale::current().tag()),
    )));
    let names = formatter.shortStandaloneWeekdaySymbols();
    Ok([1, 2, 3, 4, 5, 6, 0]
        .map(|index| names.objectAtIndex(index).to_string())
        .into())
}

pub fn time(time: Time) -> Result<String, String> {
    let formatter = NSDateFormatter::new();
    formatter.setTimeStyle(NSDateFormatterStyle::ShortStyle);
    formatter.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
    let date = NSDate::dateWithTimeIntervalSince1970(
        f64::from(time.hour()) * 3600.0 + f64::from(time.minute()) * 60.0,
    );
    Ok(formatter.stringFromDate(&date).to_string())
}
