use std::rc::Rc;

use crate::locale::tr;

use jiff::civil::Time;
use windows_reactor::*;

use super::AppState;
use crate::{
    mode::Mode,
    schedule::{Rule, Schedule, Weekday},
};

#[derive(Clone)]
pub(crate) struct ScheduleInput {
    pub state: Rc<AppState>,
    pub schedule: Schedule,
    pub active: bool,
    pub opened: Callback<Callback<()>>,
    pub editing: Callback<bool>,
}

impl PartialEq for ScheduleInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
            && self.schedule == other.schedule
            && self.active == other.active
    }
}

struct Draft {
    index: Option<usize>,
    days: [bool; 7],
    time: Option<Time>,
    mode: Mode,
}

pub(crate) struct Editor {
    state: Rc<AppState>,
    schedule: Schedule,
    draft: Option<Draft>,
    editing: Callback<bool>,
    error: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Enabled(bool),
    ApplyOnLaunch(bool),
    Edit(usize),
    Add,
    Cancel,
    Day(usize, bool),
    Time(Option<TimeSpan>),
    Mode(Option<usize>),
    Save,
    Remove,
    ClearError,
}

impl Component for Editor {
    type Input = ScheduleInput;
    type Message = Message;

    fn create(input: &Self::Input, context: &ComponentContext<Self>) -> Self {
        _ = input.opened.call(context.sender().message(Message::Cancel));
        Self {
            state: Rc::clone(&input.state),
            schedule: input.schedule.clone(),
            draft: None,
            editing: input.editing.clone(),
            error: String::new(),
        }
    }

    fn input_changed(&mut self, input: &Self::Input, _context: &ComponentContext<Self>) {
        self.schedule = input.schedule.clone();
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        self.error.clear();
        match message {
            Message::Enabled(value) => {
                let mut schedule = self.schedule.clone();
                schedule.enabled = value;
                self.save(schedule);
            }
            Message::ApplyOnLaunch(value) => {
                let mut schedule = self.schedule.clone();
                schedule.apply_on_launch = value;
                self.save(schedule);
            }
            Message::Edit(index) => {
                if let Some(rule) = self.schedule.rules.get(index) {
                    self.draft = Some(Draft {
                        index: Some(index),
                        days: Weekday::ALL.map(|day| rule.days.contains(&day)),
                        time: Some(rule.time),
                        mode: rule.mode,
                    });
                }
            }
            Message::Add => {
                self.draft = Some(Draft {
                    index: None,
                    days: [true, true, true, true, true, false, false],
                    time: None,
                    mode: Mode::Dark,
                })
            }
            Message::Cancel => self.draft = None,
            Message::Day(index, value) => {
                if let Some(draft) = &mut self.draft
                    && let Some(day) = draft.days.get_mut(index)
                {
                    *day = value;
                }
            }
            Message::Time(Some(value)) => {
                if let Some(draft) = &mut self.draft {
                    let minutes = value.whole_minutes();
                    match Time::new((minutes / 60) as i8, (minutes % 60) as i8, 0, 0) {
                        Ok(time) => draft.time = Some(time),
                        Err(error) => self.error = error.to_string(),
                    }
                }
            }
            Message::Time(None) => {}
            Message::Mode(value) => {
                if let Some(draft) = &mut self.draft {
                    draft.mode = if value == Some(1) {
                        Mode::Dark
                    } else {
                        Mode::Light
                    };
                }
            }
            Message::Save => {
                let Some(draft) = &self.draft else { return };
                let Some(time) = draft.time else {
                    self.error = tr!("Choose a time.").into();
                    return;
                };
                let days = Weekday::ALL
                    .iter()
                    .zip(draft.days)
                    .filter_map(|(day, enabled)| enabled.then_some(*day))
                    .collect::<Vec<_>>();
                if days.is_empty() {
                    self.error = tr!("Choose at least one day.").into();
                    return;
                }
                let rule = Rule {
                    days,
                    time,
                    mode: draft.mode,
                };
                let mut schedule = self.schedule.clone();
                if let Some(index) = draft.index {
                    let Some(saved) = schedule.rules.get_mut(index) else {
                        self.error = tr!("This rule has been removed.").into();
                        return;
                    };
                    *saved = rule;
                } else {
                    schedule.rules.push(rule);
                }
                if self.save(schedule) {
                    self.draft = None;
                }
            }
            Message::Remove => {
                let Some(index) = self.draft.as_ref().and_then(|draft| draft.index) else {
                    return;
                };
                let mut schedule = self.schedule.clone();
                if index < schedule.rules.len() {
                    schedule.rules.remove(index);
                    if self.save(schedule) {
                        self.draft = None;
                    }
                }
            }
            Message::ClearError => {}
        }
        _ = self.editing.call(self.draft.is_some());
    }

    fn view(&self, input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        if !input.active {
            return View::empty();
        }
        let body = if self.draft.is_some() {
            self.editor(context)
        } else {
            let rules = self.schedule.rules.iter().enumerate().map(|(index, rule)| {
                let time = super::locale::time(rule.time).unwrap_or_else(|error| error);
                let detail = tr!(
                    "{days}, {mode}",
                    days = crate::locale::days(&rule.days).unwrap_or_else(|error| error),
                    mode = rule.mode.label()
                );
                KeyedView::new(
                    index.to_string(),
                    Button::new()
                        .style(ButtonStyle::Subtle)
                        .horizontal_alignment(HorizontalAlignment::Stretch)
                        .horizontal_content_alignment(HorizontalAlignment::Stretch)
                        .on_click(context.message(Message::Edit(index)))
                        .content(
                            StackPanel::new().spacing(4.0).children((
                                TextBlock::new()
                                    .text(time)
                                    .font_size(24.0)
                                    .font_weight(FontWeight::SEMI_BOLD),
                                TextBlock::new()
                                    .text(detail)
                                    .text_wrapping(TextWrapping::Wrap),
                            )),
                        ),
                )
            });
            StackPanel::new().spacing(16.0).children((
                ToggleSwitch::new()
                    .header(tr!("Automatic switching"))
                    .is_on(self.schedule.enabled)
                    .on_toggled(context.callback(Message::Enabled)),
                ToggleSwitch::new()
                    .header(tr!("Apply schedule on launch"))
                    .is_on(self.schedule.apply_on_launch)
                    .on_toggled(context.callback(Message::ApplyOnLaunch)),
                Button::new()
                    .on_click(context.message(Message::Add))
                    .content(tr!("Add rule")),
                StackPanel::new().spacing(8.0).keyed_children(rules),
            ))
        };
        let content = StackPanel::new().spacing(20.0).children((
            TextBlock::new()
                .text(
                    if self
                        .draft
                        .as_ref()
                        .is_some_and(|draft| draft.index.is_none())
                    {
                        tr!("New rule")
                    } else {
                        tr!("Schedule")
                    },
                )
                .font_size(28.0)
                .font_weight(FontWeight::SEMI_BOLD),
            body,
            InfoBar::new()
                .is_open(!self.error.is_empty())
                .severity(InfoBarSeverity::Error)
                .message(self.error.clone())
                .on_closed(context.message(Message::ClearError)),
        ));
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(Border::new().padding(24.0).content(content))
    }
}

impl Editor {
    fn save(&mut self, schedule: Schedule) -> bool {
        let mut config = self.state.config();
        config.schedule = schedule.clone();
        match self.state.save_config(config, self.state.launch_at_login()) {
            Ok(()) => {
                self.schedule = schedule;
                true
            }
            Err(error) => {
                self.error = error;
                false
            }
        }
    }

    fn editor(&self, context: &ViewContext<Self>) -> View {
        let Some(draft) = &self.draft else {
            return View::empty();
        };
        let (names, clock) = match (super::locale::weekdays(), super::locale::clock()) {
            (Ok(names), Ok(clock)) => (names, clock),
            (Err(error), _) | (_, Err(error)) => {
                return InfoBar::new()
                    .is_open(true)
                    .severity(InfoBarSeverity::Error)
                    .message(error)
                    .into();
            }
        };
        let days = names.iter().enumerate().map(|(index, name)| {
            KeyedView::new(
                index.to_string(),
                CheckBox::new()
                    .grid_row((index / 4) as i32)
                    .grid_column((index % 4) as i32)
                    .is_checked(draft.days[index])
                    .on_is_checked_changed(
                        context.callback(move |value| Message::Day(index, value)),
                    )
                    .content(name.clone()),
            )
        });
        let remove: View = if draft.index.is_some() {
            Button::new()
                .on_click(context.message(Message::Remove))
                .content(tr!("Delete rule"))
        } else {
            View::empty()
        };
        let time = draft.time.map_or_else(View::empty, |time| {
            TextBlock::new()
                .text(super::locale::time(time).unwrap_or_else(|error| error))
                .font_size(24.0)
                .into()
        });
        StackPanel::new().spacing(16.0).children((
            time,
            TimePicker::new()
                .clock_identifier(clock)
                .header(if draft.index.is_some() {
                    tr!("Choose another time")
                } else {
                    tr!("Time")
                })
                .on_selected_time_changed(context.callback(Message::Time)),
            TextBlock::new().text(tr!("Days")),
            Grid::new()
                .columns([GridLength::STAR; 4])
                .rows([GridLength::Auto; 2])
                .column_spacing(8.0)
                .row_spacing(8.0)
                .keyed_children(days),
            ComboBox::new()
                .header(tr!("Appearance"))
                .items_source([tr!("Light"), tr!("Dark")])
                .selected_index(Some(usize::from(draft.mode == Mode::Dark)))
                .on_selection_changed(context.callback(Message::Mode)),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    Button::new()
                        .style(ButtonStyle::Accent)
                        .is_enabled(draft.time.is_some() && draft.days.iter().any(|day| *day))
                        .on_click(context.message(Message::Save))
                        .content(tr!("Save rule")),
                    Button::new()
                        .on_click(context.message(Message::Cancel))
                        .content(tr!("Cancel")),
                    remove,
                )),
        ))
    }
}
