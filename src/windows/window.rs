use std::{collections::HashMap, rc::Rc};

use crate::locale::tr;

use windows_reactor::*;

use super::{
    AppState, appearance,
    profile::{Editor, ProfileInput},
    schedule::{Editor as ScheduleEditor, ScheduleInput},
    settings::{Settings, SettingsInput},
};
use crate::{config::Profile, mode::Mode};

#[derive(Clone)]
pub(crate) struct WindowInput(pub Rc<AppState>);

impl PartialEq for WindowInput {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Event {
    Activate,
    Changed,
    Focus(bool),
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Appearance,
    Profiles,
    Schedule,
    Settings,
}

pub(crate) struct Main {
    state: Rc<AppState>,
    page: Page,
    editing: Option<(Option<String>, Profile)>,
    selected: Option<String>,
    back: Option<Callback<()>>,
    schedule_back: Option<Callback<()>>,
    schedule_editing: bool,
    focused: bool,
    mode: Result<Mode, String>,
    status: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Runtime(Event),
    Navigate(Option<String>),
    Edit(Option<String>),
    ProfileChanged(String),
    Reorder(Vec<String>),
    Run(String),
    Back,
    BindBack(Callback<()>),
    BindScheduleBack(Callback<()>),
    ScheduleEditing(bool),
    Finished,
    Mode(Option<usize>),
    ClearError,
}

impl Component for Main {
    type Input = WindowInput;
    type Message = Message;

    fn create(input: &Self::Input, context: &ComponentContext<Self>) -> Self {
        input
            .0
            .window_opened(context.sender().callback(Message::Runtime));
        Self {
            state: Rc::clone(&input.0),
            page: Page::Appearance,
            editing: None,
            selected: None,
            back: None,
            schedule_back: None,
            schedule_editing: false,
            focused: true,
            mode: input.0.system_mode(),
            status: String::new(),
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Runtime(Event::Activate) => {
                if !context.window().request_activate() {
                    self.status = tr!("Could not activate the window.").into();
                }
                self.mode = self.state.system_mode();
            }
            Message::Runtime(Event::Changed) => self.mode = self.state.system_mode(),
            Message::Runtime(Event::Focus(value)) => self.focused = value,
            Message::Navigate(Some(tag)) => {
                self.page = match tag.as_str() {
                    "profiles" => Page::Profiles,
                    "schedule" => Page::Schedule,
                    "settings" => Page::Settings,
                    _ => Page::Appearance,
                }
            }
            Message::Navigate(None) => {}
            Message::Edit(name) => {
                let profile = match &name {
                    Some(name) => match self.state.config().profile(name).cloned() {
                        Some(profile) => profile,
                        None => {
                            self.status = tr!("This configuration has been removed.").into();
                            return;
                        }
                    },
                    None => Profile::default(),
                };
                self.selected = name.clone();
                self.editing = Some((name, profile));
            }
            Message::ProfileChanged(name) => {
                self.selected = Some(name.clone());
                if let Some(profile) = self.state.config().profile(&name).cloned() {
                    self.editing = Some((Some(name), profile));
                }
            }
            Message::Reorder(tags) => {
                let order: HashMap<_, _> = tags
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| (name, index))
                    .collect();
                let mut config = self.state.config();
                config.profiles.sort_by_key(|profile| order[&profile.name]);
                self.status = self
                    .state
                    .save_config(config, self.state.launch_at_login())
                    .err()
                    .unwrap_or_default();
            }
            Message::Run(name) => {
                self.status = self.state.run_profile(&name).err().unwrap_or_default();
            }
            Message::Back => {
                let back = if self.page == Page::Schedule {
                    &self.schedule_back
                } else {
                    &self.back
                };
                if let Some(back) = back {
                    _ = back.call(());
                }
            }
            Message::BindBack(back) => self.back = Some(back),
            Message::BindScheduleBack(back) => self.schedule_back = Some(back),
            Message::ScheduleEditing(editing) => self.schedule_editing = editing,
            Message::Finished => {
                self.editing = None;
                self.back = None;
            }
            Message::Mode(Some(index)) => {
                self.state
                    .select(if index == 1 { Mode::Dark } else { Mode::Light });
                self.mode = self.state.system_mode();
            }
            Message::Mode(None) => {}
            Message::ClearError => self.status.clear(),
        }
    }

    fn view(&self, _input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        context.window_title("pola");
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(680.0, 480.0)
                .constraints(WindowConstraints {
                    min_width: Some(420.0),
                    min_height: Some(360.0),
                    ..Default::default()
                }),
        );
        let config = self.state.config();
        let editor = self
            .editing
            .as_ref()
            .map_or_else(View::empty, |(name, profile)| {
                View::component::<Editor>(ProfileInput {
                    state: Rc::clone(&self.state),
                    name: name.clone(),
                    profile: profile.clone(),
                    active: self.page == Page::Profiles,
                    opened: context.callback(Message::BindBack),
                    changed: context.callback(Message::ProfileChanged),
                    finished: context.message(Message::Finished),
                })
            });
        let content = Grid::new().children((
            if self.page == Page::Appearance {
                appearance::view(
                    &self.mode,
                    self.state.client.state().next.as_ref(),
                    context.callback(Message::Mode),
                )
            } else {
                View::empty()
            },
            if self.page == Page::Profiles && self.editing.is_none() {
                self.profiles(context)
            } else {
                View::empty()
            },
            editor,
            View::component::<ScheduleEditor>(ScheduleInput {
                state: Rc::clone(&self.state),
                schedule: config.schedule.clone(),
                active: self.page == Page::Schedule,
                opened: context.callback(Message::BindScheduleBack),
                editing: context.callback(Message::ScheduleEditing),
            }),
            View::component::<Settings>(SettingsInput {
                state: Rc::clone(&self.state),
                config,
                active: self.page == Page::Settings,
                focused: self.focused,
            }),
        ));
        let error = self.mode.as_ref().err().unwrap_or(&self.status);
        let body = Grid::new()
            .rows([GridLength::Auto, GridLength::STAR])
            .children((
                InfoBar::new()
                    .is_open(!error.is_empty())
                    .severity(InfoBarSeverity::Error)
                    .message(error.clone())
                    .on_closed(context.message(Message::ClearError)),
                Border::new().grid_row(1).content(content),
            ));
        let appearance_selected = self.page == Page::Appearance;
        let navigation = NavigationView::new()
            .pane_display_mode(NavigationViewPaneDisplayMode::Auto)
            .is_back_button_visible(NavigationViewBackButtonVisible::Collapsed)
            .is_settings_visible(false)
            .always_show_header(false)
            .on_selected_tag_changed(context.callback(Message::Navigate))
            .menu_items([
                (
                    "appearance",
                    NavigationViewItem::new()
                        .tag("appearance")
                        .is_selected(appearance_selected)
                        .icon(SymbolIcon::new().symbol(Symbol::Pictures))
                        .content(tr!("Appearance")),
                ),
                (
                    "profiles",
                    NavigationViewItem::new()
                        .tag("profiles")
                        .is_selected(self.page == Page::Profiles)
                        .icon(SymbolIcon::new().symbol(Symbol::List))
                        .content(tr!("Configurations")),
                ),
                (
                    "schedule",
                    NavigationViewItem::new()
                        .tag("schedule")
                        .is_selected(self.page == Page::Schedule)
                        .icon(SymbolIcon::new().symbol(Symbol::Calendar))
                        .content(tr!("Schedule")),
                ),
            ])
            .footer_menu_items([(
                "settings",
                NavigationViewItem::new()
                    .tag("settings")
                    .is_selected(self.page == Page::Settings)
                    .icon(SymbolIcon::new().symbol(Symbol::Setting))
                    .content(tr!("Settings")),
            )])
            .content(body)
            .grid_row(1);
        let title = TitleBar::new()
            .title("pola")
            .is_back_button_visible(
                (self.page == Page::Profiles && self.editing.is_some())
                    || (self.page == Page::Schedule && self.schedule_editing),
            )
            .is_back_button_enabled(if self.page == Page::Schedule {
                self.schedule_back.is_some()
            } else {
                self.back.is_some()
            })
            .on_back_requested(context.message(Message::Back));
        Grid::new()
            .rows([GridLength::Auto, GridLength::STAR])
            .children((title, navigation))
    }
}

impl Main {
    fn profiles(&self, context: &mut ViewContext<Self>) -> View {
        let config = self.state.config();
        let selected = config
            .profiles
            .iter()
            .position(|profile| Some(&profile.name) == self.selected.as_ref());
        let profiles = config.profiles.into_iter().map(|profile| {
            let name = profile.name;
            KeyedView::new(
                name.clone(),
                ListViewItem::new().tag(name.clone()).content(
                    Grid::new()
                        .columns([GridLength::STAR, GridLength::Auto])
                        .column_spacing(8.0)
                        .children((
                            Button::new()
                                .style(ButtonStyle::Subtle)
                                .horizontal_alignment(HorizontalAlignment::Stretch)
                                .horizontal_content_alignment(HorizontalAlignment::Left)
                                .on_click(context.message(Message::Edit(Some(name.clone()))))
                                .content(name.clone()),
                            Button::new()
                                .grid_column(1)
                                .is_enabled(!self.state.client.state().busy)
                                .on_click(context.message(Message::Run(name)))
                                .content(tr!("Run")),
                        )),
                ),
            )
        });
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(28.0).content(
                    StackPanel::new().spacing(16.0).children((
                        TextBlock::new()
                            .text(tr!("Configurations"))
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        ListView::new()
                            .selected_index(selected)
                            .can_drag_items(true)
                            .can_reorder_items(true)
                            .allow_drop(true)
                            .on_reordered(context.callback(Message::Reorder))
                            .items(profiles),
                        Button::new()
                            .on_click(context.message(Message::Edit(None)))
                            .content(tr!("New configuration")),
                    )),
                ),
            )
    }
}

impl Drop for Main {
    fn drop(&mut self) {
        self.state.window_closed();
    }
}
