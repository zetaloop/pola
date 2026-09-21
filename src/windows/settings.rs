use std::{rc::Rc, str::FromStr};

use jiff::civil::Time;
use windows_reactor::*;

use crate::{
    config::Config,
    mode::Mode,
    schedule::{Rule, Schedule, Weekday},
};

use super::AppState;

const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

const DAY_NAMES: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Section {
    General,
    Schedule,
}

#[derive(Clone)]
pub(crate) struct SettingsInput {
    pub state: Rc<AppState>,
    pub config: Config,
    pub section: Section,
}

impl PartialEq for SettingsInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
            && self.config == other.config
            && self.section == other.section
    }
}

struct Draft {
    launch_at_login: bool,
    schedule_enabled: bool,
    apply_on_launch: bool,
    shortcut: String,
    rules: Vec<RuleDraft>,
}

struct RuleDraft {
    days: [bool; 7],
    time: String,
    mode: Mode,
}

impl Draft {
    fn from_config(config: Config, launch_at_login: bool) -> Self {
        Self {
            launch_at_login,
            schedule_enabled: config.schedule.enabled,
            apply_on_launch: config.schedule.apply_on_launch,
            shortcut: config.shortcut,
            rules: config
                .schedule
                .rules
                .into_iter()
                .map(|rule| RuleDraft {
                    days: WEEKDAYS.map(|day| rule.days.contains(&day)),
                    time: rule.time.strftime("%H:%M").to_string(),
                    mode: rule.mode,
                })
                .collect(),
        }
    }

    fn config(&self, mut config: Config) -> Result<Config, String> {
        let rules = self
            .rules
            .iter()
            .map(|rule| {
                let days = WEEKDAYS
                    .into_iter()
                    .zip(rule.days)
                    .filter_map(|(day, enabled)| enabled.then_some(day))
                    .collect::<Vec<_>>();
                if days.is_empty() {
                    return Err("Each schedule rule needs at least one day.".into());
                }
                let time = Time::from_str(&rule.time)
                    .map_err(|error| format!("Invalid time '{}': {error}", rule.time))?;
                Ok(Rule {
                    days,
                    time,
                    mode: rule.mode,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        config.shortcut = self.shortcut.clone();
        config.schedule = Schedule {
            enabled: self.schedule_enabled,
            apply_on_launch: self.apply_on_launch,
            rules,
        };
        Ok(config)
    }
}

pub(crate) struct Settings {
    state: Rc<AppState>,
    draft: Draft,
    saved: Config,
    status: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    LaunchAtLogin(bool),
    ScheduleEnabled(bool),
    ApplyOnLaunch(bool),
    Shortcut(String),
    AddRule,
    RemoveRule(usize),
    RuleDay(usize, usize, bool),
    RuleTime(usize, String),
    RuleMode(usize, Option<usize>),
    Save,
}

impl Component for Settings {
    type Input = SettingsInput;
    type Message = Message;

    fn create(input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self {
            state: Rc::clone(&input.state),
            draft: Draft::from_config(input.config.clone(), input.state.launch_at_login()),
            saved: input.config.clone(),
            status: String::new(),
        }
    }

    fn input_changed(&mut self, input: &Self::Input, _context: &ComponentContext<Self>) {
        if self.saved != input.config {
            self.draft = Draft::from_config(input.config.clone(), input.state.launch_at_login());
            self.saved = input.config.clone();
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        self.status.clear();
        match message {
            Message::LaunchAtLogin(value) => self.draft.launch_at_login = value,
            Message::ScheduleEnabled(value) => self.draft.schedule_enabled = value,
            Message::ApplyOnLaunch(value) => self.draft.apply_on_launch = value,
            Message::Shortcut(value) => self.draft.shortcut = value,
            Message::AddRule => self.draft.rules.push(RuleDraft {
                days: [true, true, true, true, true, false, false],
                time: "18:00".into(),
                mode: Mode::Dark,
            }),
            Message::RemoveRule(index) => {
                if index < self.draft.rules.len() {
                    self.draft.rules.remove(index);
                }
            }
            Message::RuleDay(rule, day, value) => {
                if let Some(rule) = self.draft.rules.get_mut(rule)
                    && let Some(enabled) = rule.days.get_mut(day)
                {
                    *enabled = value;
                }
            }
            Message::RuleTime(index, value) => {
                if let Some(rule) = self.draft.rules.get_mut(index) {
                    rule.time = value;
                }
            }
            Message::RuleMode(index, selected) => {
                if let Some(rule) = self.draft.rules.get_mut(index) {
                    rule.mode = if selected == Some(1) {
                        Mode::Dark
                    } else {
                        Mode::Light
                    };
                }
            }
            Message::Save => match self
                .draft
                .config(self.state.config())
                .and_then(|config| self.state.save_config(config, self.draft.launch_at_login))
            {
                Ok(()) => self.status = "Saved".into(),
                Err(error) => self.status = error,
            },
        }
    }

    fn view(&self, input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        let content = match input.section {
            Section::General => self.general_view(context),
            Section::Schedule => self.schedule_view(context),
        };
        Grid::new()
            .rows([GridLength::STAR, GridLength::Auto])
            .children((
                Border::new().content(content),
                StackPanel::new()
                    .grid_row(1)
                    .orientation(Orientation::Horizontal)
                    .margin(20.0)
                    .spacing(12.0)
                    .children((
                        Button::new()
                            .on_click(context.message(Message::Save))
                            .content("Save"),
                        TextBlock::new().text(self.status.clone()),
                    )),
            ))
    }
}

impl Settings {
    fn general_view(&self, context: &mut ViewContext<Self>) -> View {
        Border::new().padding(Thickness::uniform(20.0)).content(
            StackPanel::new().spacing(14.0).children((
                CheckBox::new()
                    .is_checked(self.draft.launch_at_login)
                    .on_is_checked_changed(context.callback(Message::LaunchAtLogin))
                    .content("Launch at login"),
                CheckBox::new()
                    .is_checked(self.draft.schedule_enabled)
                    .on_is_checked_changed(context.callback(Message::ScheduleEnabled))
                    .content("Enable schedule"),
                CheckBox::new()
                    .is_checked(self.draft.apply_on_launch)
                    .on_is_checked_changed(context.callback(Message::ApplyOnLaunch))
                    .content("Apply schedule on launch"),
                TextBox::new()
                    .header("Shortcut")
                    .text(self.draft.shortcut.clone())
                    .on_text_changed(context.callback(Message::Shortcut)),
            )),
        )
    }

    fn schedule_view(&self, context: &mut ViewContext<Self>) -> View {
        let rows = self.draft.rules.iter().enumerate().map(|(index, rule)| {
            let days = StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .keyed_children((0..7).map(|day| {
                    KeyedView::new(
                        format!("day-{index}-{day}"),
                        CheckBox::new()
                            .is_checked(rule.days[day])
                            .on_is_checked_changed(
                                context.callback(move |value| Message::RuleDay(index, day, value)),
                            )
                            .content(DAY_NAMES[day]),
                    )
                }));
            KeyedView::new(
                format!("rule-{index}"),
                Border::new().padding(Thickness::uniform(8.0)).content(
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(8.0)
                        .children((
                            days,
                            TextBox::new()
                                .width(88.0)
                                .text(rule.time.clone())
                                .placeholder_text("HH:MM")
                                .on_text_changed(
                                    context.callback(move |value| Message::RuleTime(index, value)),
                                ),
                            ComboBox::new()
                                .width(100.0)
                                .items_source(["Light", "Dark"])
                                .selected_index(Some(usize::from(rule.mode == Mode::Dark)))
                                .on_selection_changed(
                                    context.callback(move |value| Message::RuleMode(index, value)),
                                ),
                            Button::new()
                                .on_click(context.message(Message::RemoveRule(index)))
                                .content("Remove"),
                        )),
                ),
            )
        });
        let content = StackPanel::new().spacing(8.0).children((
            StackPanel::new().keyed_children(rows),
            Button::new()
                .on_click(context.message(Message::AddRule))
                .content("Add schedule"),
        ));
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new()
                    .padding(Thickness::uniform(12.0))
                    .content(content),
            )
    }
}
