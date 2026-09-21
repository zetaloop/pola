use std::path::PathBuf;

use windows_reactor::*;

use crate::{config::Action, mode::Mode};

#[derive(Clone, PartialEq)]
pub struct Input {
    pub action: Action,
    pub finished: Callback<Option<Action>>,
}

pub struct Editor {
    draft: Action,
    finished: Callback<Option<Action>>,
    keys: Vec<usize>,
    next: usize,
    preview_failed: bool,
    error: String,
}

#[derive(Clone)]
pub enum Message {
    Kind(Option<usize>),
    Color(Option<usize>),
    Path(String),
    Drop(DroppedData),
    ImageFailed,
    Wait(bool),
    AddArgument,
    RemoveArgument(usize),
    Argument(usize, String),
    Save,
    Cancel,
    ClearError,
}

impl Component for Editor {
    type Input = Input;
    type Message = Message;

    fn create(input: &Input, _context: &ComponentContext<Self>) -> Self {
        let count = match &input.action {
            Action::Command(command) => command.args.len(),
            _ => 0,
        };
        Self {
            draft: input.action.clone(),
            finished: input.finished.clone(),
            keys: (0..count).collect(),
            next: count,
            preview_failed: false,
            error: String::new(),
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        self.error.clear();
        match message {
            Message::Kind(Some(index)) => {
                if let Some(action) = Action::choices().get(index)
                    && std::mem::discriminant(action) != std::mem::discriminant(&self.draft)
                {
                    self.draft = action.clone();
                    self.keys.clear();
                    self.preview_failed = false;
                }
            }
            Message::Color(Some(index)) => {
                self.draft = Action::Color {
                    mode: if index == 0 { Mode::Light } else { Mode::Dark },
                };
            }
            Message::Kind(None) | Message::Color(None) => {}
            Message::Path(value) => self.set_path(value),
            Message::Drop(data) => match dropped_file(data) {
                Ok(path) => self.set_path(path.to_string_lossy().into_owned()),
                Err(error) => self.error = error,
            },
            Message::ImageFailed => self.preview_failed = true,
            Message::Wait(wait) => {
                if let Action::Command(command) = &mut self.draft {
                    command.wait = wait;
                }
            }
            Message::AddArgument => {
                if let Action::Command(command) = &mut self.draft {
                    command.args.push(String::new());
                    self.keys.push(self.next);
                    self.next += 1;
                }
            }
            Message::RemoveArgument(key) => {
                if let Action::Command(command) = &mut self.draft
                    && let Some(index) = self.keys.iter().position(|id| *id == key)
                {
                    command.args.remove(index);
                    self.keys.remove(index);
                }
            }
            Message::Argument(key, value) => {
                if let Action::Command(command) = &mut self.draft
                    && let Some(index) = self.keys.iter().position(|id| *id == key)
                {
                    command.args[index] = value;
                }
            }
            Message::Save => match self.draft.validate() {
                Ok(()) => {
                    _ = self.finished.call(Some(self.draft.clone()));
                }
                Err(error) => self.error = error.to_string(),
            },
            Message::Cancel => {
                _ = self.finished.call(None);
            }
            Message::ClearError => {}
        }
    }

    fn view(&self, _input: &Input, context: &mut ViewContext<Self>) -> View {
        let choices = Action::choices();
        let selected = choices.iter().position(|action| {
            std::mem::discriminant(action) == std::mem::discriminant(&self.draft)
        });
        let fields: View = match &self.draft {
            Action::Color { mode } => RadioButtons::new()
                .items_source(["Light", "Dark"])
                .selected_index(usize::from(*mode == Mode::Dark))
                .on_selection_changed(context.callback(Message::Color))
                .into(),
            Action::Wallpaper { path } | Action::Theme { path } => {
                let preview: View = if matches!(self.draft, Action::Wallpaper { .. })
                    && !path.as_os_str().is_empty()
                {
                    if self.preview_failed {
                        TextBlock::new()
                            .text("Wallpaper preview unavailable")
                            .into()
                    } else {
                        match Image::new().source_file(path) {
                            Ok(image) => image
                                .stretch(Stretch::Uniform)
                                .max_height(240.0)
                                .on_failed(context.message(Message::ImageFailed))
                                .into(),
                            Err(error) => TextBlock::new()
                                .text(error.to_string())
                                .text_wrapping(TextWrapping::Wrap)
                                .into(),
                        }
                    }
                } else {
                    View::empty()
                };
                StackPanel::new().spacing(12.0).children((
                    preview,
                    self.file_input("File", &path.to_string_lossy(), context),
                ))
            }
            Action::Command(command) => {
                let arguments =
                    command.args.iter().zip(&self.keys).enumerate().map(
                        |(index, (value, &key))| {
                            KeyedView::new(
                                key.to_string(),
                                Grid::new()
                                    .columns([GridLength::STAR, GridLength::Auto])
                                    .column_spacing(8.0)
                                    .children((
                                        TextBox::new()
                                            .header(format!("Argument {}", index + 1))
                                            .text(value.clone())
                                            .accepts_return(true)
                                            .text_wrapping(TextWrapping::Wrap)
                                            .on_text_changed(context.callback(move |value| {
                                                Message::Argument(key, value)
                                            })),
                                        Button::new()
                                            .grid_column(1)
                                            .vertical_alignment(VerticalAlignment::Bottom)
                                            .on_click(context.message(Message::RemoveArgument(key)))
                                            .content("Remove"),
                                    )),
                            )
                        },
                    );
                StackPanel::new().spacing(12.0).children((
                    self.file_input("Program", &command.program, context),
                    StackPanel::new().spacing(8.0).keyed_children(arguments),
                    Button::new()
                        .on_click(context.message(Message::AddArgument))
                        .content("Add argument"),
                    CheckBox::new()
                        .is_checked(command.wait)
                        .on_is_checked_changed(context.callback(Message::Wait))
                        .content("Continue after the program exits"),
                ))
            }
        };
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(28.0).content(
                    StackPanel::new().spacing(20.0).children((
                        TextBlock::new()
                            .text("Action")
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        ComboBox::new()
                            .items_source(choices.iter().map(Action::title))
                            .selected_index(selected)
                            .on_selection_changed(context.callback(Message::Kind)),
                        fields,
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
                                    .on_click(context.message(Message::Save))
                                    .content("Save"),
                                Button::new()
                                    .on_click(context.message(Message::Cancel))
                                    .content("Cancel"),
                            )),
                    )),
                ),
            )
    }
}

impl Editor {
    fn set_path(&mut self, value: String) {
        match &mut self.draft {
            Action::Wallpaper { path } | Action::Theme { path } => *path = value.into(),
            Action::Command(command) => command.program = value,
            Action::Color { .. } => {}
        }
        self.preview_failed = false;
    }

    fn file_input(&self, title: &str, value: &str, context: &ViewContext<Self>) -> View {
        Border::new()
            .drop_policy(DragDropPolicy::new().storage_items(
                DragDropAction::new(DragDropOperation::Copy).caption("Use this file"),
            ))
            .on_drop(context.callback(Message::Drop))
            .content(
                TextBox::new()
                    .header(title)
                    .text(value)
                    .placeholder_text("Drop a file or enter its path")
                    .on_text_changed(context.callback(Message::Path)),
            )
    }
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
