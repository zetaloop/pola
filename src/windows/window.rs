use std::rc::Rc;

use crate::locale::tr;

use windows_reactor::*;

use super::{
    AppState,
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
}

#[derive(Clone, PartialEq)]
enum Page {
    Appearance,
    Profiles,
    Schedule,
    Settings,
    Profile {
        name: Option<String>,
        profile: Profile,
    },
}

pub(crate) struct Main {
    state: Rc<AppState>,
    page: Page,
    pane_open: bool,
    mode: Result<Mode, String>,
    status: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Runtime(Event),
    Navigate(Option<String>),
    Edit(Option<String>),
    Run(String),
    Back,
    Pane(bool),
    TogglePane,
    Mode(Option<usize>),
    Exit,
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
            pane_open: true,
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
                self.page = Page::Profile { name, profile };
            }
            Message::Run(name) => {
                self.status = self.state.run_profile(&name).err().unwrap_or_default();
            }
            Message::Back => self.page = Page::Profiles,
            Message::Pane(value) => self.pane_open = value,
            Message::TogglePane => self.pane_open = !self.pane_open,
            Message::Mode(Some(index)) => {
                self.state
                    .select(if index == 1 { Mode::Dark } else { Mode::Light });
                self.mode = self.state.system_mode();
            }
            Message::Mode(None) => {}
            Message::Exit => self.state.exit(),
            Message::ClearError => self.status.clear(),
        }
    }

    fn view(&self, _input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        context.window_title("pola");
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(960.0, 640.0)
                .constraints(WindowConstraints {
                    min_width: Some(640.0),
                    min_height: Some(480.0),
                    ..Default::default()
                }),
        );
        let config = self.state.config();
        let content = match &self.page {
            Page::Appearance => self.appearance(context),
            Page::Profiles => self.profiles(context),
            Page::Profile { name, profile } => View::component::<Editor>(ProfileInput {
                state: Rc::clone(&self.state),
                name: name.clone(),
                profile: profile.clone(),
                finished: context.message(Message::Back),
            }),
            Page::Schedule => View::component::<ScheduleEditor>(ScheduleInput {
                state: Rc::clone(&self.state),
                schedule: config.schedule,
            }),
            Page::Settings => View::component::<Settings>(SettingsInput {
                state: Rc::clone(&self.state),
                config,
            }),
        };
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
            .is_pane_open(self.pane_open)
            .on_is_pane_open_changed(context.callback(Message::Pane))
            .is_pane_toggle_button_visible(false)
            .is_back_button_visible(NavigationViewBackButtonVisible::Collapsed)
            .is_settings_visible(false)
            .always_show_header(false)
            .open_pane_length(220.0)
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
                        .is_selected(matches!(self.page, Page::Profiles | Page::Profile { .. }))
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
            .pane_footer(
                Button::new()
                    .margin(Thickness::uniform(12.0))
                    .on_click(context.message(Message::Exit))
                    .content(tr!("Quit pola")),
            )
            .content(body)
            .grid_row(1);
        let title = TitleBar::new()
            .title("pola")
            .preferred_height(WindowTitleBarHeight::Tall)
            .is_back_button_visible(matches!(self.page, Page::Profile { .. }))
            .is_back_button_enabled(true)
            .on_back_requested(context.message(Message::Back))
            .is_pane_toggle_button_visible(true)
            .on_pane_toggle_requested(context.message(Message::TogglePane));
        Grid::new()
            .rows([GridLength::Auto, GridLength::STAR])
            .children((title, navigation))
    }
}

impl Main {
    fn profiles(&self, context: &mut ViewContext<Self>) -> View {
        let profiles = self.state.config().profiles.into_iter().map(|profile| {
            let name = profile.name;
            KeyedView::new(
                name.clone(),
                Grid::new()
                    .columns([GridLength::STAR, GridLength::Auto])
                    .column_spacing(8.0)
                    .children((
                        Button::new()
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
                        StackPanel::new().spacing(8.0).keyed_children(profiles),
                        Button::new()
                            .on_click(context.message(Message::Edit(None)))
                            .content(tr!("New configuration")),
                    )),
                ),
            )
    }

    fn appearance(&self, context: &mut ViewContext<Self>) -> View {
        let next = self
            .state
            .client
            .state()
            .next
            .map(|event| crate::locale::next(&event).unwrap_or_else(|error| error))
            .unwrap_or_default();
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(28.0).content(
                    StackPanel::new().spacing(24.0).children((
                        TextBlock::new()
                            .text(tr!("Appearance"))
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        RadioButtons::new()
                            .items_source([tr!("Light"), tr!("Dark")])
                            .max_columns(2)
                            .selected_index(
                                self.mode
                                    .as_ref()
                                    .ok()
                                    .map(|mode| usize::from(*mode == Mode::Dark)),
                            )
                            .on_selection_changed(context.callback(Message::Mode)),
                        TextBlock::new()
                            .text(next)
                            .text_wrapping(TextWrapping::Wrap),
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
