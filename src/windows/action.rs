use std::{path::PathBuf, rc::Rc};

use crate::locale::tr;

use windows_reactor::*;

use super::AppState;
use crate::{config::Action, mode::Mode};

#[derive(Clone)]
pub struct Input {
    pub state: Rc<AppState>,
    pub name: String,
    pub index: Option<usize>,
    pub action: Action,
    pub active: bool,
    pub finished: Callback<()>,
}

impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
            && self.name == other.name
            && self.index == other.index
            && self.action == other.action
            && self.active == other.active
    }
}

pub struct Editor {
    state: Rc<AppState>,
    name: String,
    index: Option<usize>,
    draft: Action,
    finished: Callback<()>,
    field: ElementRef<TextBox>,
    focus: Option<usize>,
    keys: Vec<usize>,
    next: usize,
    preview_failed: bool,
    error: String,
}

#[derive(Clone)]
pub enum Message {
    Choose,
    Picked(Result<Option<PathBuf>, String>),
    Focused(bool),
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
            state: Rc::clone(&input.state),
            name: input.name.clone(),
            index: input.index,
            field: ElementRef::new(),
            focus: None,
            draft: input.action.clone(),
            finished: input.finished.clone(),
            keys: (0..count).collect(),
            next: count,
            preview_failed: false,
            error: String::new(),
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        if !matches!(message, Message::Focused(_) | Message::ImageFailed) {
            self.error.clear();
        }
        match message {
            Message::Choose => {
                if !windows_pickers::OpenFilePicker::new().request(context, |result| {
                    Message::Picked(result.map_err(|error| error.to_string()))
                }) {
                    self.error = tr!("Could not open the file picker.").into();
                }
            }
            Message::Picked(Ok(Some(path))) => self.set_path(path.to_string_lossy().into_owned()),
            Message::Picked(Ok(None)) => {}
            Message::Picked(Err(error)) => self.error = error,
            Message::Focused(success) => {
                self.focus = None;
                if !success {
                    self.error = tr!("Could not focus the input.").into();
                }
            }
            Message::Color(Some(index)) => {
                self.draft = Action::Color {
                    mode: if index == 0 { Mode::Light } else { Mode::Dark },
                };
            }
            Message::Color(None) => {}
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
                    self.focus = Some(self.next);
                    self.next += 1;
                }
            }
            Message::RemoveArgument(key) => {
                if let Action::Command(command) = &mut self.draft
                    && let Some(index) = self.keys.iter().position(|id| *id == key)
                {
                    command.args.remove(index);
                    self.keys.remove(index);
                    self.focus = self
                        .keys
                        .get(index.min(self.keys.len().saturating_sub(1)))
                        .copied();
                }
            }
            Message::Argument(key, value) => {
                if let Action::Command(command) = &mut self.draft
                    && let Some(index) = self.keys.iter().position(|id| *id == key)
                {
                    command.args[index] = value;
                }
            }
            Message::Save => match self.save() {
                Ok(()) => {
                    _ = self.finished.call(());
                }
                Err(error) => self.error = error,
            },
            Message::Cancel => {
                _ = self.finished.call(());
            }
            Message::ClearError => {}
        }
    }

    fn view(&self, input: &Input, context: &mut ViewContext<Self>) -> View {
        if !input.active {
            return View::empty();
        }
        if let Some(key) = self.focus {
            let field = self.field.clone();
            let completed = context.callback(Message::Focused);
            context.use_effect("argument-focus", key, move || {
                let result = completed.clone();
                if !field.request_focus_result(move |value| {
                    _ = result.call(matches!(value, Ok(true)));
                }) {
                    _ = completed.call(false);
                }
                None
            });
        }
        let fields: View = match &self.draft {
            Action::Color { mode } => RadioButtons::new()
                .items_source([tr!("Light"), tr!("Dark")])
                .selected_index(usize::from(*mode == Mode::Dark))
                .on_selection_changed(context.callback(Message::Color))
                .into(),
            Action::Wallpaper { path } | Action::Theme { path } => {
                let preview: View = if matches!(self.draft, Action::Wallpaper { .. })
                    && !path.as_os_str().is_empty()
                {
                    if self.preview_failed {
                        TextBlock::new()
                            .text(tr!("Wallpaper preview unavailable"))
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
                    self.file_input(tr!("File"), &path.to_string_lossy(), context),
                ))
            }
            Action::Command(command) => {
                let arguments = command.args.iter().zip(&self.keys).enumerate().map(
                    |(index, (value, &key))| {
                        let field = TextBox::new()
                            .header(tr!("Argument {number}", number = index + 1))
                            .text(value.clone())
                            .accepts_return(true)
                            .text_wrapping(TextWrapping::Wrap)
                            .on_text_changed(
                                context.callback(move |value| Message::Argument(key, value)),
                            );
                        let field = if self.focus == Some(key) {
                            field.element_ref(&self.field)
                        } else {
                            field
                        };
                        KeyedView::new(
                            key.to_string(),
                            Grid::new()
                                .columns([GridLength::STAR, GridLength::Auto])
                                .column_spacing(8.0)
                                .children((
                                    field,
                                    Button::new()
                                        .grid_column(1)
                                        .vertical_alignment(VerticalAlignment::Bottom)
                                        .on_click(context.message(Message::RemoveArgument(key)))
                                        .content(tr!("Remove")),
                                )),
                        )
                    },
                );
                StackPanel::new().spacing(12.0).children((
                    self.file_input(tr!("Program"), &command.program, context),
                    StackPanel::new().spacing(8.0).keyed_children(arguments),
                    Button::new()
                        .on_click(context.message(Message::AddArgument))
                        .content(tr!("Add argument")),
                    CheckBox::new()
                        .is_checked(command.wait)
                        .on_is_checked_changed(context.callback(Message::Wait))
                        .content(tr!("Continue after the program exits")),
                ))
            }
        };
        ScrollViewer::new()
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .content(
                Border::new().padding(28.0).content(
                    StackPanel::new().spacing(20.0).children((
                        TextBlock::new()
                            .text(self.draft.title())
                            .font_size(28.0)
                            .font_weight(FontWeight::SEMI_BOLD),
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
                                    .content(tr!("Save")),
                                Button::new()
                                    .on_click(context.message(Message::Cancel))
                                    .content(tr!("Cancel")),
                            )),
                    )),
                ),
            )
    }
}

impl Editor {
    fn save(&self) -> Result<(), String> {
        let mut profile = self
            .state
            .config()
            .profile(&self.name)
            .cloned()
            .ok_or(tr!("This configuration has been removed."))?;
        if let Some(index) = self.index {
            *profile
                .actions
                .get_mut(index)
                .ok_or(tr!("This action has been removed."))? = self.draft.clone();
        } else {
            profile.actions.push(self.draft.clone());
        }
        self.state.save_profile(Some(&self.name), profile)
    }

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
                DragDropAction::new(DragDropOperation::Copy).caption(tr!("Use this file")),
            ))
            .on_drop(context.callback(Message::Drop))
            .content(
                Grid::new()
                    .columns([GridLength::STAR, GridLength::Auto])
                    .column_spacing(8.0)
                    .children((
                        TextBox::new()
                            .header(title)
                            .text(value)
                            .placeholder_text(tr!("Drop a file or enter its path"))
                            .on_text_changed(context.callback(Message::Path)),
                        Button::new()
                            .grid_column(1)
                            .vertical_alignment(VerticalAlignment::Bottom)
                            .on_click(context.message(Message::Choose))
                            .content(tr!("Choose…")),
                    )),
            )
    }
}

fn dropped_file(data: DroppedData) -> Result<PathBuf, String> {
    let DroppedData::StorageItems(items) = data else {
        return Err(tr!("Drop a file from File Explorer.").into());
    };
    let [item] = items.as_slice() else {
        return Err(tr!("Drop one file at a time.").into());
    };
    let path = PathBuf::from(&item.path);
    if !path.is_file() {
        return Err(tr!("The dropped item must be a file.").into());
    }
    Ok(path)
}
