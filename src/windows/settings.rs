use std::{path::PathBuf, rc::Rc, str::FromStr};

use jiff::civil::Time;
use windows_pickers::OpenFilePicker;
use windows_reactor::*;

use crate::{
    config::{Command, Config, General, Profile},
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

#[derive(Clone)]
pub(crate) struct SettingsInput(pub Rc<AppState>);

impl PartialEq for SettingsInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

struct Draft {
    schedule_enabled: bool,
    apply_on_launch: bool,
    shortcut: String,
    rules: Vec<RuleDraft>,
    light: ProfileDraft,
    dark: ProfileDraft,
}

struct RuleDraft {
    days: [bool; 7],
    time: String,
    mode: Mode,
}

struct ProfileDraft {
    wallpaper: String,
    commands: Vec<Command>,
}

impl Draft {
    fn from_config(config: Config) -> Self {
        Self {
            schedule_enabled: config.schedule.enabled,
            apply_on_launch: config.schedule.apply_on_launch,
            shortcut: config.general.shortcut,
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
            light: ProfileDraft::from_profile(config.light),
            dark: ProfileDraft::from_profile(config.dark),
        }
    }

    fn config(&self) -> Result<Config, String> {
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

        Ok(Config {
            general: General {
                shortcut: self.shortcut.clone(),
            },
            schedule: Schedule {
                enabled: self.schedule_enabled,
                apply_on_launch: self.apply_on_launch,
                rules,
            },
            light: self.light.profile(),
            dark: self.dark.profile(),
        })
    }

    fn profile(&self, mode: Mode) -> &ProfileDraft {
        match mode {
            Mode::Light => &self.light,
            Mode::Dark => &self.dark,
        }
    }

    fn profile_mut(&mut self, mode: Mode) -> &mut ProfileDraft {
        match mode {
            Mode::Light => &mut self.light,
            Mode::Dark => &mut self.dark,
        }
    }
}

impl ProfileDraft {
    fn from_profile(profile: Profile) -> Self {
        Self {
            wallpaper: profile
                .wallpaper
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            commands: profile.commands,
        }
    }

    fn profile(&self) -> Profile {
        Profile {
            wallpaper: (!self.wallpaper.is_empty()).then(|| PathBuf::from(&self.wallpaper)),
            commands: self
                .commands
                .iter()
                .filter(|command| !command.program.is_empty())
                .cloned()
                .collect(),
        }
    }
}

pub(crate) struct Settings {
    state: Rc<AppState>,
    draft: Draft,
    status: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Activate,
    ScheduleEnabled(bool),
    ApplyOnLaunch(bool),
    Shortcut(String),
    AddRule,
    RemoveRule(usize),
    RuleDay(usize, usize, bool),
    RuleTime(usize, String),
    RuleMode(usize, Option<usize>),
    Wallpaper(Mode, String),
    PickWallpaper(Mode),
    WallpaperPicked(Mode, Result<Option<PathBuf>, String>),
    AddCommand(Mode),
    RemoveCommand(Mode, usize),
    Program(Mode, usize, String),
    AddArgument(Mode, usize),
    RemoveArgument(Mode, usize, usize),
    Argument(Mode, usize, usize, String),
    Save,
}

impl Component for Settings {
    type Input = SettingsInput;
    type Message = Message;

    fn create(input: &Self::Input, context: &ComponentContext<Self>) -> Self {
        input
            .0
            .settings_opened(context.sender().callback(|()| Message::Activate));
        Self {
            state: Rc::clone(&input.0),
            draft: Draft::from_config(input.0.config()),
            status: String::new(),
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Activate => {
                if !context.window().request_activate() {
                    self.status = "Could not activate the settings window.".into();
                }
            }
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
            Message::Wallpaper(mode, value) => self.draft.profile_mut(mode).wallpaper = value,
            Message::PickWallpaper(mode) => {
                let requested = OpenFilePicker::new()
                    .title("Choose wallpaper")
                    .filter_extensions("Images", ["png", "jpg", "jpeg", "webp", "bmp"])
                    .filter_all()
                    .request(context, move |result| {
                        Message::WallpaperPicked(mode, result.map_err(|error| error.to_string()))
                    });
                if !requested {
                    self.status = "Another file picker is already open.".into();
                }
            }
            Message::WallpaperPicked(mode, Ok(Some(path))) => {
                self.draft.profile_mut(mode).wallpaper = path.to_string_lossy().into_owned();
                self.status.clear();
            }
            Message::WallpaperPicked(_, Ok(None)) => {}
            Message::WallpaperPicked(_, Err(error)) => self.status = error,
            Message::AddCommand(mode) => {
                self.draft.profile_mut(mode).commands.push(Command {
                    program: String::new(),
                    args: Vec::new(),
                });
            }
            Message::RemoveCommand(mode, index) => {
                let commands = &mut self.draft.profile_mut(mode).commands;
                if index < commands.len() {
                    commands.remove(index);
                }
            }
            Message::Program(mode, index, value) => {
                if let Some(command) = self.draft.profile_mut(mode).commands.get_mut(index) {
                    command.program = value;
                }
            }
            Message::AddArgument(mode, command) => {
                if let Some(command) = self.draft.profile_mut(mode).commands.get_mut(command) {
                    command.args.push(String::new());
                }
            }
            Message::RemoveArgument(mode, command, argument) => {
                if let Some(command) = self.draft.profile_mut(mode).commands.get_mut(command)
                    && argument < command.args.len()
                {
                    command.args.remove(argument);
                }
            }
            Message::Argument(mode, command, argument, value) => {
                if let Some(command) = self.draft.profile_mut(mode).commands.get_mut(command)
                    && let Some(argument) = command.args.get_mut(argument)
                {
                    *argument = value;
                }
            }
            Message::Save => match self
                .draft
                .config()
                .and_then(|config| self.state.save_config(config))
            {
                Ok(()) => self.status = "Saved".into(),
                Err(error) => self.status = error,
            },
        }
    }

    fn view(&self, _input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(760.0, 620.0),
        );

        let tabs = TabView::new().is_add_tab_button_visible(false).tab_items([
            KeyedView::new(
                "general",
                TabViewItem::new()
                    .header("General")
                    .is_closable(false)
                    .content(self.general_view(context)),
            ),
            KeyedView::new(
                "schedule",
                TabViewItem::new()
                    .header("Schedule")
                    .is_closable(false)
                    .content(self.schedule_view(context)),
            ),
            KeyedView::new(
                "light",
                TabViewItem::new()
                    .header("Light")
                    .is_closable(false)
                    .content(self.profile_view(context, Mode::Light)),
            ),
            KeyedView::new(
                "dark",
                TabViewItem::new()
                    .header("Dark")
                    .is_closable(false)
                    .content(self.profile_view(context, Mode::Dark)),
            ),
        ]);

        context.window_frame(
            "pola",
            StackPanel::new().spacing(12.0).children((
                tabs,
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(12.0)
                    .children((
                        Button::new()
                            .on_click(context.message(Message::Save))
                            .content("Save"),
                        TextBlock::new().text(self.status.clone()),
                    )),
            )),
        )
    }
}

impl Settings {
    fn general_view(&self, context: &mut ViewContext<Self>) -> View {
        Border::new().padding(Thickness::uniform(20.0)).content(
            StackPanel::new().spacing(14.0).children((
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
                    let callback =
                        context.callback(move |value| Message::RuleDay(index, day, value));
                    KeyedView::new(
                        format!("day-{index}-{day}"),
                        CheckBox::new()
                            .is_checked(rule.days[day])
                            .on_is_checked_changed(callback)
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

    fn profile_view(&self, context: &mut ViewContext<Self>, mode: Mode) -> View {
        let profile = self.draft.profile(mode);
        let commands = profile.commands.iter().enumerate().map(|(index, command)| {
            let arguments = command
                .args
                .iter()
                .enumerate()
                .map(|(argument_index, argument)| {
                    KeyedView::new(
                        format!("arg-{index}-{argument_index}"),
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(8.0)
                            .children((
                                TextBox::new()
                                    .width(430.0)
                                    .text(argument.clone())
                                    .placeholder_text("Argument")
                                    .on_text_changed(context.callback(move |value| {
                                        Message::Argument(mode, index, argument_index, value)
                                    })),
                                Button::new()
                                    .on_click(context.message(Message::RemoveArgument(
                                        mode,
                                        index,
                                        argument_index,
                                    )))
                                    .content("Remove"),
                            )),
                    )
                });

            KeyedView::new(
                format!("command-{index}"),
                Border::new().padding(Thickness::uniform(8.0)).content(
                    StackPanel::new().spacing(8.0).children((
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(8.0)
                            .children((
                                TextBox::new()
                                    .width(430.0)
                                    .text(command.program.clone())
                                    .placeholder_text("Program")
                                    .on_text_changed(context.callback(move |value| {
                                        Message::Program(mode, index, value)
                                    })),
                                Button::new()
                                    .on_click(context.message(Message::RemoveCommand(mode, index)))
                                    .content("Remove"),
                            )),
                        StackPanel::new().spacing(6.0).keyed_children(arguments),
                        Button::new()
                            .on_click(context.message(Message::AddArgument(mode, index)))
                            .content("Add argument"),
                    )),
                ),
            )
        });

        let content = StackPanel::new().spacing(12.0).children((
            TextBlock::new().text("Wallpaper"),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    TextBox::new()
                        .width(500.0)
                        .text(profile.wallpaper.clone())
                        .on_text_changed(
                            context.callback(move |value| Message::Wallpaper(mode, value)),
                        ),
                    Button::new()
                        .on_click(context.message(Message::PickWallpaper(mode)))
                        .content("Choose…"),
                )),
            TextBlock::new().text("Commands"),
            StackPanel::new().spacing(8.0).keyed_children(commands),
            Button::new()
                .on_click(context.message(Message::AddCommand(mode)))
                .content("Add command"),
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

impl Drop for Settings {
    fn drop(&mut self) {
        self.state.settings_closed();
    }
}
