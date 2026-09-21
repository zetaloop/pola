use std::{
    collections::{BTreeSet, HashMap},
    rc::Rc,
};

use windows_reactor::*;

use super::{AppState, action};
use crate::{
    config::{Action, Profile},
    locale::tr,
    mode::Mode,
};

#[derive(Clone)]
pub(crate) struct ProfileInput {
    pub state: Rc<AppState>,
    pub name: Option<String>,
    pub profile: Profile,
    pub active: bool,
    pub opened: Callback<Callback<()>>,
    pub changed: Callback<String>,
    pub finished: Callback<()>,
}

impl PartialEq for ProfileInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
            && self.name == other.name
            && self.profile == other.profile
            && self.active == other.active
    }
}

pub(crate) struct Editor {
    state: Rc<AppState>,
    profile: Profile,
    key: Option<String>,
    name: String,
    when: BTreeSet<Mode>,
    changed: Callback<String>,
    finished: Callback<()>,
    keys: Vec<usize>,
    next: usize,
    editing: Option<(Option<usize>, Action)>,
    error: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Name(String),
    CommitName,
    When(Mode, bool),
    Add(String),
    Edit(usize),
    CloseAction,
    Remove(usize),
    Reorder(Vec<String>),
    Delete,
    Create,
    Run,
    Back,
    ClearError,
}

impl Component for Editor {
    type Input = ProfileInput;
    type Message = Message;

    fn create(input: &ProfileInput, context: &ComponentContext<Self>) -> Self {
        _ = input.opened.call(context.sender().message(Message::Back));
        let count = input.profile.actions.len();
        Self {
            state: Rc::clone(&input.state),
            profile: input.profile.clone(),
            key: input.name.clone(),
            name: input.profile.name.clone(),
            when: input.profile.when.clone(),
            changed: input.changed.clone(),
            finished: input.finished.clone(),
            keys: (0..count).collect(),
            next: count,
            editing: None,
            error: String::new(),
        }
    }

    fn input_changed(&mut self, input: &Self::Input, _context: &ComponentContext<Self>) {
        if !input.active {
            self.commit_name();
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        match message {
            Message::Name(value) => self.name = value,
            Message::CommitName => {
                self.commit_name();
            }
            Message::When(mode, enabled) => {
                if enabled {
                    self.when.insert(mode);
                } else {
                    self.when.remove(&mode);
                }
                let mut profile = self.profile.clone();
                profile.when = self.when.clone();
                self.save(profile);
            }
            Message::Add(title) => {
                if self.commit_name()
                    && let Some(action) = Action::choices()
                        .into_iter()
                        .find(|action| action.title() == title)
                {
                    self.editing = Some((None, action));
                }
            }
            Message::Edit(key) => {
                if self.commit_name()
                    && let Some(index) = self.keys.iter().position(|id| *id == key)
                {
                    self.editing = Some((Some(index), self.profile.actions[index].clone()));
                }
            }
            Message::CloseAction => {
                self.editing = None;
                if let Some(profile) = self
                    .key
                    .as_deref()
                    .and_then(|key| self.state.config().profile(key).cloned())
                {
                    while self.keys.len() < profile.actions.len() {
                        self.keys.push(self.next);
                        self.next += 1;
                    }
                    self.profile = profile;
                }
            }
            Message::Remove(key) => {
                if let Some(index) = self.keys.iter().position(|id| *id == key) {
                    let mut profile = self.profile.clone();
                    profile.actions.remove(index);
                    if self.save(profile) {
                        self.keys.remove(index);
                    }
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
                    .iter()
                    .copied()
                    .zip(self.profile.actions.iter().cloned())
                    .collect();
                actions.sort_by_key(|(key, _)| order[&key.to_string()]);
                let (keys, actions) = actions.into_iter().unzip();
                let mut profile = self.profile.clone();
                profile.actions = actions;
                if self.save(profile) {
                    self.keys = keys;
                }
            }
            Message::Delete => {
                if let Some(key) = &self.key {
                    let mut config = self.state.config();
                    config.profiles.retain(|profile| &profile.name != key);
                    match self.state.save_config(config, self.state.launch_at_login()) {
                        Ok(()) => {
                            _ = self.finished.call(());
                        }
                        Err(error) => self.error = error,
                    }
                }
            }
            Message::Create => {
                self.save(Profile {
                    name: self.name.clone(),
                    ..Profile::default()
                });
            }
            Message::Run => {
                if self.commit_name()
                    && let Some(key) = &self.key
                {
                    self.error = self.state.run_profile(key).err().unwrap_or_default();
                }
            }
            Message::Back => {
                if self.editing.take().is_none() && self.commit_name() {
                    _ = self.finished.call(());
                }
            }
            Message::ClearError => self.error.clear(),
        }
    }

    fn view(&self, input: &ProfileInput, context: &mut ViewContext<Self>) -> View {
        if let Some((index, action)) = &self.editing {
            return View::component::<action::Editor>(action::Input {
                state: Rc::clone(&self.state),
                name: self.profile.name.clone(),
                index: *index,
                action: action.clone(),
                active: input.active,
                finished: context.message(Message::CloseAction),
            });
        }
        if !input.active {
            return View::empty();
        }
        let body = if self.key.is_none() {
            Button::new()
                .style(ButtonStyle::Accent)
                .is_enabled(!self.name.trim().is_empty())
                .on_click(context.message(Message::Create))
                .content(tr!("Create configuration"))
        } else {
            let actions = self
                .profile
                .actions
                .iter()
                .zip(&self.keys)
                .map(|(action, &key)| {
                    KeyedView::new(
                        key.to_string(),
                        ListViewItem::new()
                            .tag(key.to_string())
                            .content(
                                Button::new()
                                    .style(ButtonStyle::Subtle)
                                    .horizontal_alignment(HorizontalAlignment::Stretch)
                                    .horizontal_content_alignment(HorizontalAlignment::Left)
                                    .on_click(context.message(Message::Edit(key)))
                                    .content(
                                        TextBlock::new()
                                            .text(action.summary())
                                            .text_trimming(TextTrimming::CharacterEllipsis),
                                    ),
                            )
                            .menu(Menu::new(
                                [MenuItem::item("remove", tr!("Remove"))],
                                context.callback(move |_| Message::Remove(key)),
                            )),
                    )
                });
            let add = DropDownButton::new()
                .grid_column(1)
                .content(tr!("Add action"))
                .menu(Menu::new(
                    Action::choices()
                        .iter()
                        .enumerate()
                        .map(|(index, action)| MenuItem::item(index.to_string(), action.title())),
                    context.callback(Message::Add),
                ));
            StackPanel::new().spacing(16.0).children((
                TextBlock::new().text(tr!("Run when switching to")),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(16.0)
                    .keyed_children([Mode::Light, Mode::Dark].map(|mode| {
                        KeyedView::new(
                            mode.to_string(),
                            CheckBox::new()
                                .is_checked(self.when.contains(&mode))
                                .on_is_checked_changed(
                                    context.callback(move |enabled| Message::When(mode, enabled)),
                                )
                                .content(mode.label()),
                        )
                    })),
                Grid::new()
                    .columns([GridLength::STAR, GridLength::Auto])
                    .column_spacing(12.0)
                    .children((
                        TextBlock::new()
                            .text(tr!("Actions"))
                            .font_size(20.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        add,
                    )),
                ListView::new()
                    .selection_mode(ListViewSelectionMode::None)
                    .can_drag_items(true)
                    .can_reorder_items(true)
                    .allow_drop(true)
                    .on_reordered(context.callback(Message::Reorder))
                    .items(actions),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        Button::new()
                            .is_enabled(!self.state.client.state().busy)
                            .on_click(context.message(Message::Run))
                            .content(tr!("Run")),
                        Button::new()
                            .on_click(context.message(Message::Delete))
                            .content(tr!("Delete configuration")),
                    )),
            ))
        };
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(24.0).content(
                    StackPanel::new().spacing(20.0).children((
                        TextBlock::new()
                            .text(if self.key.is_none() {
                                tr!("New configuration")
                            } else {
                                tr!("Configuration")
                            })
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        Border::new()
                            .on_lost_focus(context.callback(|_| Message::CommitName))
                            .content(
                                TextBox::new()
                                    .header(tr!("Name"))
                                    .text(self.name.clone())
                                    .on_text_changed(context.callback(Message::Name)),
                            ),
                        body,
                        InfoBar::new()
                            .is_open(!self.error.is_empty())
                            .severity(InfoBarSeverity::Error)
                            .message(self.error.clone())
                            .on_closed(context.message(Message::ClearError)),
                    )),
                ),
            )
    }
}

impl Editor {
    fn commit_name(&mut self) -> bool {
        if self.key.is_none() || self.name == self.profile.name {
            return true;
        }
        let mut profile = self.profile.clone();
        profile.name = self.name.clone();
        self.save(profile)
    }

    fn save(&mut self, profile: Profile) -> bool {
        match self
            .state
            .save_profile(self.key.as_deref(), profile.clone())
        {
            Ok(()) => {
                self.key = Some(profile.name.clone());
                _ = self.changed.call(profile.name.clone());
                self.profile = profile;
                self.error.clear();
                true
            }
            Err(error) => {
                self.error = error;
                false
            }
        }
    }
}
