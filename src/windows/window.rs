use std::rc::Rc;

use jiff::Zoned;
use windows_reactor::*;

use super::{
    AppState,
    profile::{Editor, ProfileInput},
    schedule::{Editor as ScheduleEditor, ScheduleInput},
    settings::{Settings, SettingsInput},
};
use crate::mode::Mode;

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

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Appearance,
    Schedule,
    Settings,
    Profile(Mode),
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
    Edit(Mode),
    Back,
    Pane(bool),
    TogglePane,
    Mode(bool),
    Exit,
    ClearError,
    ImageFailed(Mode),
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
                    self.status = "Could not activate the window.".into();
                }
                self.mode = self.state.system_mode();
            }
            Message::Runtime(Event::Changed) => self.mode = self.state.system_mode(),
            Message::Navigate(Some(tag)) => {
                self.page = match tag.as_str() {
                    "schedule" => Page::Schedule,
                    "settings" => Page::Settings,
                    _ => Page::Appearance,
                }
            }
            Message::Navigate(None) => {}
            Message::Edit(mode) => self.page = Page::Profile(mode),
            Message::Back => self.page = Page::Appearance,
            Message::Pane(value) => self.pane_open = value,
            Message::TogglePane => self.pane_open = !self.pane_open,
            Message::Mode(dark) => {
                self.state
                    .select(if dark { Mode::Dark } else { Mode::Light });
                self.mode = self.state.system_mode();
            }
            Message::Exit => self.state.exit(),
            Message::ClearError => self.status.clear(),
            Message::ImageFailed(mode) => {
                self.status = format!("Could not load the {mode} wallpaper preview.")
            }
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
        let content = match self.page {
            Page::Appearance => self.appearance(context),
            Page::Profile(mode) => View::component::<Editor>(ProfileInput {
                state: Rc::clone(&self.state),
                mode,
                profile: config.profile(mode).clone(),
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
        let appearance_selected = matches!(self.page, Page::Appearance | Page::Profile(_));
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
                        .content("Appearance"),
                ),
                (
                    "schedule",
                    NavigationViewItem::new()
                        .tag("schedule")
                        .is_selected(self.page == Page::Schedule)
                        .icon(SymbolIcon::new().symbol(Symbol::Calendar))
                        .content("Schedule"),
                ),
            ])
            .footer_menu_items([(
                "settings",
                NavigationViewItem::new()
                    .tag("settings")
                    .is_selected(self.page == Page::Settings)
                    .icon(SymbolIcon::new().symbol(Symbol::Setting))
                    .content("Settings"),
            )])
            .pane_footer(
                Button::new()
                    .margin(Thickness::uniform(12.0))
                    .on_click(context.message(Message::Exit))
                    .content("Quit pola"),
            )
            .content(body)
            .grid_row(1);
        let title = TitleBar::new()
            .title("pola")
            .preferred_height(WindowTitleBarHeight::Tall)
            .is_back_button_visible(matches!(self.page, Page::Profile(_)))
            .is_back_button_enabled(true)
            .on_back_requested(context.message(Message::Back))
            .is_pane_toggle_button_visible(true)
            .on_pane_toggle_requested(context.message(Message::TogglePane))
            .right_header(
                ToggleSwitch::new()
                    .is_enabled(self.mode.is_ok())
                    .is_on(self.mode == Ok(Mode::Dark))
                    .on_content("Dark")
                    .off_content("Light")
                    .on_toggled(context.callback(Message::Mode)),
            );
        Grid::new()
            .rows([GridLength::Auto, GridLength::STAR])
            .children((title, navigation))
    }
}

impl Main {
    fn appearance(&self, context: &mut ViewContext<Self>) -> View {
        let config = self.state.config();
        let cards = [Mode::Light, Mode::Dark]
            .into_iter()
            .enumerate()
            .map(|(index, mode)| {
                let profile = config.profile(mode);
                let image: View = match &profile.wallpaper {
                    Some(path) => match Image::new().source_file(path) {
                        Ok(image) => image
                            .on_failed(context.message(Message::ImageFailed(mode)))
                            .stretch(Stretch::UniformToFill)
                            .height(180.0)
                            .into(),
                        Err(error) => TextBlock::new()
                            .text(error.to_string())
                            .text_wrapping(TextWrapping::Wrap)
                            .into(),
                    },
                    None => TextBlock::new()
                        .text("Uses the current wallpaper")
                        .text_wrapping(TextWrapping::Wrap)
                        .into(),
                };
                let mut description = if self.mode == Ok(mode) {
                    "Active".to_owned()
                } else {
                    String::new()
                };
                if !profile.commands.is_empty() {
                    if !description.is_empty() {
                        description.push('\n');
                    }
                    description.push_str(&match profile.commands.len() {
                        1 => "1 command".into(),
                        count => format!("{count} commands"),
                    });
                }
                KeyedView::new(
                    mode.to_string(),
                    Border::new()
                        .grid_column(index as i32)
                        .background(ThemeBrush::CardBackground)
                        .border_brush(ThemeBrush::CardStroke)
                        .border_thickness(Thickness::uniform(1.0))
                        .corner_radius(CornerRadius::uniform(8.0))
                        .padding(20.0)
                        .content(
                            StackPanel::new().spacing(16.0).children((
                                TextBlock::new()
                                    .text(mode.to_string())
                                    .font_size(20.0)
                                    .font_weight(FontWeight::SEMI_BOLD),
                                image,
                                TextBlock::new().text(description),
                                Button::new()
                                    .on_click(context.message(Message::Edit(mode)))
                                    .content("Edit appearance"),
                            )),
                        ),
                )
            });
        let next = config
            .schedule
            .next(&Zoned::now())
            .map(|event| {
                format!(
                    "Switch to {} at {}",
                    event.mode,
                    event.at.strftime("%a %H:%M")
                )
            })
            .unwrap_or_else(|| {
                if config.schedule.enabled {
                    "Add an arrangement to enable automatic switching.".into()
                } else {
                    "Schedule is off".into()
                }
            });
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(28.0).content(
                    StackPanel::new().spacing(24.0).children((
                        TextBlock::new()
                            .text("Appearance")
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        Grid::new()
                            .columns([GridLength::STAR, GridLength::STAR])
                            .column_spacing(16.0)
                            .keyed_children(cards),
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
