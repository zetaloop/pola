use std::{collections::HashMap, rc::Rc};

use windows_reactor::*;

use super::{AppState, action};
use crate::{
    config::{Action, Profile},
    mode::Mode,
};

#[derive(Clone)]
pub(crate) struct ProfileInput {
    pub state: Rc<AppState>,
    pub name: Option<String>,
    pub profile: Profile,
    pub finished: Callback<()>,
}

impl PartialEq for ProfileInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
            && self.name == other.name
            && self.profile == other.profile
    }
}

pub(crate) struct Editor {
    state: Rc<AppState>,
    draft: Profile,
    saved: Profile,
    name: Option<String>,
    finished: Callback<()>,
    keys: Vec<usize>,
    next: usize,
    editing: Option<(Option<usize>, Action)>,
    error: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Name(String),
    When(Mode, bool),
    Add,
    Edit(usize),
    Edited(Option<Action>),
    Remove(usize),
    Reorder(Vec<String>),
    Delete,
    Save,
    Cancel,
    ClearError,
}

impl Component for Editor {
    type Input = ProfileInput;
    type Message = Message;

    fn create(input: &ProfileInput, _context: &ComponentContext<Self>) -> Self {
        let count = input.profile.actions.len();
        Self {
            state: Rc::clone(&input.state),
            draft: input.profile.clone(),
            saved: input.profile.clone(),
            name: input.name.clone(),
            finished: input.finished.clone(),
            keys: (0..count).collect(),
            next: count,
            editing: None,
            error: String::new(),
        }
    }

    fn input_changed(&mut self, input: &ProfileInput, context: &ComponentContext<Self>) {
        if self.name != input.name || self.saved != input.profile {
            *self = Self::create(input, context);
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        self.error.clear();
        match message {
            Message::Name(value) => self.draft.name = value,
            Message::When(mode, enabled) => {
                self.draft.when.retain(|value| *value != mode);
                if enabled {
                    self.draft.when.insert(mode);
                }
            }
            Message::Add => self.editing = Some((None, Action::Color { mode: Mode::Light })),
            Message::Edit(key) => {
                if let Some(index) = self.keys.iter().position(|id| *id == key) {
                    self.editing = Some((Some(index), self.draft.actions[index].clone()));
                }
            }
            Message::Edited(action) => {
                if let Some((index, _)) = self.editing.take()
                    && let Some(action) = action
                {
                    match index {
                        Some(index) => self.draft.actions[index] = action,
                        None => {
                            self.draft.actions.push(action);
                            self.keys.push(self.next);
                            self.next += 1;
                        }
                    }
                }
            }
            Message::Remove(key) => {
                if let Some(index) = self.keys.iter().position(|id| *id == key) {
                    self.draft.actions.remove(index);
                    self.keys.remove(index);
                }
            }
            Message::Reorder(tags) => {
                let order: HashMap<_, _> = tags
                    .into_iter()
                    .enumerate()
                    .map(|(index, tag)| (tag, index))
                    .collect();
                let mut actions: Vec<_> = self
                    .keys
                    .drain(..)
                    .zip(self.draft.actions.drain(..))
                    .collect();
                actions.sort_by_key(|(key, _)| order[&key.to_string()]);
                (self.keys, self.draft.actions) = actions.into_iter().unzip();
            }
            Message::Delete => {
                if let Some(name) = &self.name {
                    let mut config = self.state.config();
                    config.profiles.retain(|profile| &profile.name != name);
                    match self.state.save_config(config, self.state.launch_at_login()) {
                        Ok(()) => {
                            _ = self.finished.call(());
                        }
                        Err(error) => self.error = error,
                    }
                }
            }
            Message::Save => {
                let mut config = self.state.config();
                if let Some(name) = &self.name {
                    let Some(profile) = config
                        .profiles
                        .iter_mut()
                        .find(|profile| &profile.name == name)
                    else {
                        self.error = "This configuration has been removed.".into();
                        return;
                    };
                    *profile = self.draft.clone();
                } else {
                    config.profiles.push(self.draft.clone());
                }
                match self.state.save_config(config, self.state.launch_at_login()) {
                    Ok(()) => {
                        _ = self.finished.call(());
                    }
                    Err(error) => self.error = error,
                }
            }
            Message::Cancel => {
                _ = self.finished.call(());
            }
            Message::ClearError => {}
        }
    }

    fn view(&self, _input: &ProfileInput, context: &mut ViewContext<Self>) -> View {
        if let Some((_, action)) = &self.editing {
            return View::component::<action::Editor>(action::Input {
                action: action.clone(),
                finished: context.callback(Message::Edited),
            });
        }
        let actions = self
            .draft
            .actions
            .iter()
            .zip(&self.keys)
            .map(|(action, &key)| {
                KeyedView::new(
                    key.to_string(),
                    ListViewItem::new().tag(key.to_string()).content(
                        Grid::new()
                            .columns([GridLength::STAR, GridLength::Auto])
                            .column_spacing(8.0)
                            .children((
                                Button::new()
                                    .horizontal_alignment(HorizontalAlignment::Stretch)
                                    .horizontal_content_alignment(HorizontalAlignment::Left)
                                    .on_click(context.message(Message::Edit(key)))
                                    .content(
                                        TextBlock::new()
                                            .text(action.summary())
                                            .text_trimming(TextTrimming::CharacterEllipsis),
                                    ),
                                Button::new()
                                    .grid_column(1)
                                    .on_click(context.message(Message::Remove(key)))
                                    .content("Remove"),
                            )),
                    ),
                )
            });
        let content = StackPanel::new().spacing(20.0).max_width(800.0).children((
            TextBlock::new()
                .text(self.name.as_deref().unwrap_or("New configuration"))
                .font_size(28.0)
                .font_weight(FontWeight::SEMI_BOLD),
            TextBox::new()
                .header("Name")
                .text(self.draft.name.clone())
                .on_text_changed(context.callback(Message::Name)),
            TextBlock::new().text("Run when switching to"),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(16.0)
                .keyed_children([Mode::Light, Mode::Dark].map(|mode| {
                    KeyedView::new(
                        mode.to_string(),
                        CheckBox::new()
                            .is_checked(self.draft.when.contains(&mode))
                            .on_is_checked_changed(
                                context.callback(move |enabled| Message::When(mode, enabled)),
                            )
                            .content(mode.to_string()),
                    )
                })),
            TextBlock::new()
                .text("Actions")
                .font_size(20.0)
                .font_weight(FontWeight::SEMI_BOLD),
            ListView::new()
                .selection_mode(ListViewSelectionMode::None)
                .can_drag_items(true)
                .can_reorder_items(true)
                .allow_drop(true)
                .on_reordered(context.callback(Message::Reorder))
                .items(actions),
            Button::new()
                .on_click(context.message(Message::Add))
                .content("Add action"),
        ));
        Grid::new()
            .rows([GridLength::STAR, GridLength::Auto])
            .children((
                ScrollViewer::new()
                    .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
                    .content(Border::new().padding(28.0).content(content)),
                StackPanel::new()
                    .grid_row(1)
                    .margin(Thickness::new(28.0, 0.0, 28.0, 20.0))
                    .spacing(12.0)
                    .children((
                        InfoBar::new()
                            .is_open(!self.error.is_empty())
                            .severity(InfoBarSeverity::Error)
                            .message(self.error.clone())
                            .on_closed(context.message(Message::ClearError)),
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(8.0)
                            .children((
                                Button::new()
                                    .style(ButtonStyle::Accent)
                                    .is_enabled(self.draft != self.saved)
                                    .on_click(context.message(Message::Save))
                                    .content("Save"),
                                Button::new()
                                    .on_click(context.message(Message::Cancel))
                                    .content("Cancel"),
                                Button::new()
                                    .is_enabled(self.name.is_some())
                                    .on_click(context.message(Message::Delete))
                                    .content("Delete configuration"),
                            )),
                    )),
            ))
    }
}
