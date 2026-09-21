use std::{path::PathBuf, rc::Rc};

use windows_reactor::*;

use super::AppState;
use crate::{
    config::{Command, Profile},
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
    expanded: Option<usize>,
    preview_failed: bool,
    error: String,
}

#[derive(Clone)]
pub(crate) enum Message {
    Name(String),
    When(Mode, bool),
    Delete,
    Wallpaper(String),
    WallpaperDropped(DroppedData),
    ImageFailed,
    Expand(usize, bool),
    AddCommand,
    RemoveCommand(usize),
    Program(usize, String),
    ProgramDropped(usize, DroppedData),
    AddArgument(usize),
    RemoveArgument(usize, usize),
    Argument(usize, usize, String),
    Save,
    Cancel,
    ClearError,
}

impl Component for Editor {
    type Input = ProfileInput;
    type Message = Message;

    fn create(input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self {
            state: Rc::clone(&input.state),
            draft: input.profile.clone(),
            saved: input.profile.clone(),
            name: input.name.clone(),
            finished: input.finished.clone(),
            expanded: None,
            preview_failed: false,
            error: String::new(),
        }
    }

    fn input_changed(&mut self, input: &Self::Input, context: &ComponentContext<Self>) {
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
                    self.draft.when.push(mode);
                }
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
            Message::Wallpaper(value) => {
                self.draft.wallpaper = (!value.is_empty()).then(|| PathBuf::from(value));
                self.preview_failed = false;
            }
            Message::WallpaperDropped(data) => match dropped_file(data) {
                Ok(path) => {
                    self.draft.wallpaper = Some(path);
                    self.preview_failed = false;
                }
                Err(error) => self.error = error,
            },
            Message::ImageFailed => self.preview_failed = true,
            Message::Expand(index, open) => {
                if open {
                    self.expanded = Some(index);
                } else if self.expanded == Some(index) {
                    self.expanded = None;
                }
            }
            Message::AddCommand => {
                self.expanded = Some(self.draft.commands.len());
                self.draft.commands.push(Command {
                    program: String::new(),
                    args: Vec::new(),
                });
            }
            Message::RemoveCommand(index) => {
                if index < self.draft.commands.len() {
                    self.draft.commands.remove(index);
                    self.expanded = self.expanded.and_then(|expanded| {
                        (expanded != index).then(|| expanded - usize::from(expanded > index))
                    });
                }
            }
            Message::Program(index, value) => {
                if let Some(command) = self.draft.commands.get_mut(index) {
                    command.program = value;
                }
            }
            Message::ProgramDropped(index, data) => match dropped_file(data) {
                Ok(path) => {
                    if let Some(command) = self.draft.commands.get_mut(index) {
                        command.program = path.to_string_lossy().into_owned();
                    }
                }
                Err(error) => self.error = error,
            },
            Message::AddArgument(index) => {
                if let Some(command) = self.draft.commands.get_mut(index) {
                    command.args.push(String::new());
                }
            }
            Message::RemoveArgument(index, argument) => {
                if let Some(command) = self.draft.commands.get_mut(index)
                    && argument < command.args.len()
                {
                    command.args.remove(argument);
                }
            }
            Message::Argument(index, argument, value) => {
                if let Some(command) = self.draft.commands.get_mut(index)
                    && let Some(argument) = command.args.get_mut(argument)
                {
                    *argument = value;
                }
            }
            Message::Save => {
                if self
                    .draft
                    .commands
                    .iter()
                    .any(|command| command.program.is_empty())
                {
                    self.error = "Enter a program for each command.".into();
                    return;
                }
                if self
                    .draft
                    .wallpaper
                    .as_ref()
                    .is_some_and(|path| !path.is_file())
                {
                    self.error = "The wallpaper file could not be found.".into();
                    return;
                }
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

    fn view(&self, _input: &Self::Input, context: &mut ViewContext<Self>) -> View {
        let path = self
            .draft
            .wallpaper
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let preview: View = match self
            .draft
            .wallpaper
            .as_ref()
            .filter(|_| !self.preview_failed)
        {
            Some(path) => match Image::new().source_file(path) {
                Ok(image) => image
                    .stretch(Stretch::Uniform)
                    .max_height(240.0)
                    .on_failed(context.message(Message::ImageFailed))
                    .into(),
                Err(error) => TextBlock::new()
                    .text(error.to_string())
                    .text_wrapping(TextWrapping::Wrap)
                    .into(),
            },
            None => TextBlock::new()
                .text(if self.preview_failed {
                    "Wallpaper preview unavailable"
                } else {
                    "Drop an image here"
                })
                .into(),
        };
        let commands = self
            .draft
            .commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                let title = if command.program.is_empty() {
                    "New command".to_owned()
                } else {
                    PathBuf::from(&command.program)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| command.program.clone())
                };
                let arguments = command.args.iter().enumerate().map(|(argument, value)| {
                    KeyedView::new(
                        format!("argument-{argument}"),
                        Grid::new()
                            .columns([GridLength::STAR, GridLength::Auto])
                            .column_spacing(8.0)
                            .children((
                                TextBox::new()
                                    .header(format!("Argument {}", argument + 1))
                                    .text(value.clone())
                                    .accepts_return(true)
                                    .text_wrapping(TextWrapping::Wrap)
                                    .on_text_changed(context.callback(move |value| {
                                        Message::Argument(index, argument, value)
                                    })),
                                Button::new()
                                    .grid_column(1)
                                    .vertical_alignment(VerticalAlignment::Bottom)
                                    .on_click(
                                        context.message(Message::RemoveArgument(index, argument)),
                                    )
                                    .content("Remove"),
                            )),
                    )
                });
                KeyedView::new(
                    format!("command-{index}"),
                    Expander::new()
                        .horizontal_alignment(HorizontalAlignment::Stretch)
                        .is_expanded(self.expanded == Some(index))
                        .on_is_expanded_changed(
                            context.callback(move |open| Message::Expand(index, open)),
                        )
                        .header(
                            TextBlock::new()
                                .text(title)
                                .text_trimming(TextTrimming::CharacterEllipsis),
                        )
                        .content(
                            StackPanel::new().spacing(12.0).children((
                                Border::new()
                                    .drop_policy(file_drop("Use this program"))
                                    .on_drop(
                                        context.callback(move |data| {
                                            Message::ProgramDropped(index, data)
                                        }),
                                    )
                                    .content(
                                        TextBox::new()
                                            .header("Program")
                                            .text(command.program.clone())
                                            .on_text_changed(context.callback(move |value| {
                                                Message::Program(index, value)
                                            })),
                                    ),
                                StackPanel::new().spacing(8.0).keyed_children(arguments),
                                StackPanel::new()
                                    .orientation(Orientation::Horizontal)
                                    .spacing(8.0)
                                    .children((
                                        Button::new()
                                            .on_click(context.message(Message::AddArgument(index)))
                                            .content("Add argument"),
                                        Button::new()
                                            .on_click(
                                                context.message(Message::RemoveCommand(index)),
                                            )
                                            .content("Remove command"),
                                    )),
                            )),
                        ),
                )
            });
        let content = StackPanel::new()
            .spacing(20.0)
            .max_width(800.0)
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .children((
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
                Border::new()
                    .background(ThemeBrush::CardBackground)
                    .border_brush(ThemeBrush::CardStroke)
                    .border_thickness(1.0)
                    .corner_radius(8.0)
                    .padding(20.0)
                    .drop_policy(file_drop("Use as wallpaper"))
                    .on_drop(context.callback(Message::WallpaperDropped))
                    .content(
                        StackPanel::new().spacing(16.0).children((
                            TextBlock::new()
                                .text("Wallpaper")
                                .font_size(20.0)
                                .font_weight(FontWeight::SEMI_BOLD),
                            preview,
                            TextBox::new()
                                .header("Image path")
                                .text(path)
                                .placeholder_text("Use the current wallpaper")
                                .on_text_changed(context.callback(Message::Wallpaper)),
                        )),
                    ),
                TextBlock::new()
                    .text("Commands")
                    .font_size(20.0)
                    .font_weight(FontWeight::SEMI_BOLD),
                StackPanel::new().spacing(8.0).keyed_children(commands),
                Button::new()
                    .on_click(context.message(Message::AddCommand))
                    .content("Add command"),
            ));
        let changed = self.draft != self.saved;
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
                                    .is_enabled(changed)
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

fn file_drop(caption: &str) -> DragDropPolicy {
    DragDropPolicy::new()
        .storage_items(DragDropAction::new(DragDropOperation::Copy).caption(caption))
}

fn dropped_file(data: DroppedData) -> Result<PathBuf, String> {
    let DroppedData::StorageItems(items) = data else {
        return Err("Drop a file from File Explorer.".into());
    };
    let [item] = items.as_slice() else {
        return Err("Drop one file at a time.".into());
    };
    let path = PathBuf::from(&item.path);
    if !path.is_file() {
        return Err("The dropped item must be a file.".into());
    }
    Ok(path)
}
