use jiff::{
    ToSpan, Zoned,
    civil::{Time, Weekday as JiffWeekday},
};
use serde::{Deserialize, Serialize};

use crate::mode::Mode;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    pub apply_on_launch: bool,
    pub rules: Vec<Rule>,
}

impl Schedule {
    pub fn current(&self, now: &Zoned) -> Option<Mode> {
        if !self.enabled {
            return None;
        }

        let mut latest = None;

        for days_ago in 0..=7 {
            let date = now.date().checked_sub(days_ago.days()).ok()?;

            for rule in &self.rules {
                if !rule.days.contains(&Weekday::from(date.weekday())) {
                    continue;
                }

                let at = date
                    .to_datetime(rule.time)
                    .to_zoned(now.time_zone().clone())
                    .ok()?;

                if at.timestamp() <= now.timestamp()
                    && latest
                        .as_ref()
                        .is_none_or(|(latest_at, _)| at.timestamp() >= *latest_at)
                {
                    latest = Some((at.timestamp(), rule.mode));
                }
            }
        }

        latest.map(|(_, mode)| mode)
    }

    pub fn next(&self, now: &Zoned) -> Option<Event> {
        if !self.enabled {
            return None;
        }

        let mut next = None;

        for days_ahead in 0..=7 {
            let date = now.date().checked_add(days_ahead.days()).ok()?;

            for rule in &self.rules {
                if !rule.days.contains(&Weekday::from(date.weekday())) {
                    continue;
                }

                let at = date
                    .to_datetime(rule.time)
                    .to_zoned(now.time_zone().clone())
                    .ok()?;

                if at.timestamp() > now.timestamp()
                    && next
                        .as_ref()
                        .is_none_or(|next: &Event| at.timestamp() <= next.at.timestamp())
                {
                    next = Some(Event {
                        at,
                        mode: rule.mode,
                    });
                }
            }
        }

        next
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Rule {
    pub days: Vec<Weekday>,
    pub time: Time,
    pub mode: Mode,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub at: Zoned,
    pub mode: Mode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl From<JiffWeekday> for Weekday {
    fn from(day: JiffWeekday) -> Self {
        match day {
            JiffWeekday::Monday => Self::Mon,
            JiffWeekday::Tuesday => Self::Tue,
            JiffWeekday::Wednesday => Self::Wed,
            JiffWeekday::Thursday => Self::Thu,
            JiffWeekday::Friday => Self::Fri,
            JiffWeekday::Saturday => Self::Sat,
            JiffWeekday::Sunday => Self::Sun,
        }
    }
}
