use std::{cell::RefCell, path::PathBuf, ptr};

use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSIndexSet, NSNotification, NSObject, NSObjectProtocol, NSString,
};

use super::{Delegate, file::FileInput, ui};
use crate::{
    config::{Command, Profile},
    mode::Mode,
};

struct CommandForm {
    window: Retained<NSWindow>,
    index: Option<usize>,
    program: Retained<FileInput>,
    arguments: Vec<String>,
    table: Retained<NSTableView>,
    remove: Retained<NSButton>,
    error: Retained<NSTextField>,
}

pub struct Ivars {
    owner: Weak<Delegate>,
    key: RefCell<Option<String>>,
    name: Retained<NSTextField>,
    when: Retained<NSSegmentedControl>,
    run: Retained<NSButton>,
    delete: Retained<NSButton>,
    profile: RefCell<Profile>,
    view: Retained<NSView>,
    commands: Retained<NSTableView>,
    edit: Retained<NSButton>,
    remove: Retained<NSButton>,
    error: Retained<NSTextField>,
    form: RefCell<Option<CommandForm>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Editor;

    unsafe impl NSObjectProtocol for Editor {}
    unsafe impl NSTextFieldDelegate for Editor {}
    unsafe impl NSControlTextEditingDelegate for Editor {
        #[unsafe(method(controlTextDidEndEditing:))]
        fn argument_changed(&self, notification: &NSNotification) {
            if let Some(object) = notification.object()
                && let Ok(field) = object.downcast::<NSTextField>()
            {
                if ptr::eq(&*field, &*self.ivars().name) {
                    let profile = self.ivars().profile.borrow().clone();
                    self.save(profile);
                } else if let Some(form) = self.ivars().form.borrow_mut().as_mut()
                    && let Some(argument) = form.arguments.get_mut(field.tag() as usize)
                {
                    *argument = field.stringValue().to_string();
                }
            }
        }
    }

    unsafe impl NSTableViewDataSource for Editor {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, table: &NSTableView) -> isize {
            if ptr::eq(table, &*self.ivars().commands) {
                self.ivars().profile.borrow().commands.len() as isize
            } else {
                self.ivars()
                    .form
                    .borrow()
                    .as_ref()
                    .map_or(0, |form| form.arguments.len() as isize)
            }
        }
    }

    unsafe impl NSTableViewDelegate for Editor {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(
            &self,
            table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            if ptr::eq(table, &*self.ivars().commands) {
                let profile = self.ivars().profile.borrow();
                let program = &profile.commands[row as usize].program;
                let name = std::path::Path::new(program)
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| program.into());
                let cell = ui::cell(self.mtm(), &name, None);
                cell.setToolTip(Some(&NSString::from_str(program)));
                Some(cell.into_super())
            } else {
                self.ivars().form.borrow().as_ref().map(|form| {
                    let cell = ui::cell(self.mtm(), &form.arguments[row as usize], None);
                    let field = unsafe { cell.textField() }.unwrap();
                    field.setEditable(true);
                    field.setSelectable(true);
                    field.setTag(row);
                    field.setPlaceholderString(Some(&NSString::from_str("Argument")));
                    unsafe {
                        field.setDelegate(Some(ProtocolObject::from_ref(self)));
                    }
                    if let Some(text_cell) = field.cell() {
                        text_cell.setUsesSingleLineMode(false);
                    }
                    cell.into_super()
                })
            }
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, _notification: &NSNotification) {
            self.update_selection();
        }
    }

    impl Editor {
        #[unsafe(method(whenChanged:))]
        fn when_changed(&self, _sender: &NSObject) {
            let profile = self.ivars().profile.borrow().clone();
            self.save(profile);
        }

        #[unsafe(method(runProfile:))]
        fn run_profile(&self, _sender: &NSObject) {
            let key = self.ivars().key.borrow().clone();
            if let Some(owner) = self.ivars().owner.load()
                && let Some(name) = key
            {
                match owner.run_profile(&name) {
                    Ok(()) => self.error(""),
                    Err(error) => self.error(&error),
                }
            }
        }

        #[unsafe(method(deleteProfile:))]
        fn delete_profile(&self, _sender: &NSObject) {
            let key = self.ivars().key.borrow().clone();
            if let Some(owner) = self.ivars().owner.load()
                && let Some(name) = key
            {
                let mut config = owner.config();
                config.profiles.retain(|profile| profile.name != name);
                match owner.save_config(config) {
                    Ok(()) => owner.show_profiles(),
                    Err(error) => self.error(&error),
                }
            }
        }

        #[unsafe(method(wallpaperChanged:))]
        fn wallpaper_changed(&self, input: &FileInput) {
            let path = input.value();
            if !path.is_empty()
                && NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&path))
                    .is_none()
            {
                self.error("This file could not be opened as an image.");
                return;
            }
            let mut profile = self.ivars().profile.borrow().clone();
            profile.wallpaper = (!path.is_empty()).then(|| PathBuf::from(path));
            self.save(profile);
        }

        #[unsafe(method(addCommand:))]
        fn add_command(&self, _sender: &NSObject) {
            self.edit_command(None);
        }

        #[unsafe(method(editCommand:))]
        fn edit(&self, _sender: &NSObject) {
            let row = self.ivars().commands.selectedRow();
            if row >= 0 {
                self.edit_command(Some(row as usize));
            }
        }

        #[unsafe(method(removeCommand:))]
        fn remove_command(&self, _sender: &NSObject) {
            let row = self.ivars().commands.selectedRow();
            if row < 0 {
                return;
            }
            let mut profile = self.ivars().profile.borrow().clone();
            profile.commands.remove(row as usize);
            if self.save(profile) {
                self.ivars().commands.reloadData();
                self.update_selection();
            }
        }

        #[unsafe(method(commandChanged:))]
        fn command_changed(&self, _sender: &NSObject) {
            if let Some(form) = self.ivars().form.borrow().as_ref() {
                form.error.setStringValue(&NSString::new());
            }
        }

        #[unsafe(method(addArgument:))]
        fn add_argument(&self, _sender: &NSObject) {
            self.finish_editing();
            let row = self.ivars().form.borrow_mut().as_mut().map(|form| {
                form.arguments.push(String::new());
                (form.table.clone(), form.arguments.len() - 1)
            });
            if let Some((table, row)) = row {
                table.reloadData();
                table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
                table.editColumn_row_withEvent_select(0, row as isize, None, true);
            }
        }

        #[unsafe(method(removeArgument:))]
        fn remove_argument(&self, _sender: &NSObject) {
            self.finish_editing();
            let table = self.ivars().form.borrow_mut().as_mut().and_then(|form| {
                let row = form.table.selectedRow();
                if row < 0 {
                    return None;
                }
                form.arguments.remove(row as usize);
                Some(form.table.clone())
            });
            if let Some(table) = table {
                table.reloadData();
                self.update_selection();
            }
        }

        #[unsafe(method(saveCommand:))]
        fn save_command(&self, _sender: &NSObject) {
            self.finish_editing();
            let profile = {
                let form = self.ivars().form.borrow();
                let Some(form) = form.as_ref() else {
                    return;
                };
                let program = form.program.value();
                if program.is_empty() {
                    form.error
                        .setStringValue(&NSString::from_str("Enter a program to run."));
                    return;
                }
                let command = Command {
                    program,
                    args: form.arguments.clone(),
                };
                let mut profile = self.ivars().profile.borrow().clone();
                match form.index {
                    Some(index) => profile.commands[index] = command,
                    None => profile.commands.push(command),
                }
                profile
            };
            if self.save(profile) {
                self.close_command();
                self.ivars().commands.reloadData();
                self.update_selection();
            } else if let Some(form) = self.ivars().form.borrow().as_ref() {
                form.error.setStringValue(&self.ivars().error.stringValue());
            }
        }

        #[unsafe(method(cancelCommand:))]
        fn cancel_command(&self, _sender: &NSObject) {
            self.close_command();
        }

    }
);

impl Editor {
    pub fn new(
        mtm: MainThreadMarker,
        owner: &Delegate,
        key: Option<String>,
        profile: Profile,
    ) -> Retained<Self> {
        let view = NSView::new(mtm);
        let name = NSTextField::textFieldWithString(&NSString::from_str(&profile.name), mtm);
        let when = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &objc2_foundation::NSArray::from_slice(&[
                    objc2_foundation::ns_string!("Light"),
                    objc2_foundation::ns_string!("Dark"),
                ]),
                NSSegmentSwitchTracking::SelectAny,
                None,
                Some(sel!(whenChanged:)),
                mtm,
            )
        };
        for (index, mode) in [Mode::Light, Mode::Dark].into_iter().enumerate() {
            when.setSelected_forSegment(profile.when.contains(&mode), index as isize);
        }
        let run = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Run"),
                None,
                Some(sel!(runProfile:)),
                mtm,
            )
        };
        let delete = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Delete configuration"),
                None,
                Some(sel!(deleteProfile:)),
                mtm,
            )
        };
        run.setEnabled(key.is_some());
        delete.setEnabled(key.is_some());
        let commands = ui::table(mtm, "Commands");
        let edit = unsafe {
            NSButton::buttonWithImage_target_action(
                &ui::symbol("pencil", "Edit command"),
                None,
                Some(sel!(editCommand:)),
                mtm,
            )
        };
        let remove = unsafe {
            NSButton::buttonWithImage_target_action(
                &ui::symbol("minus", "Remove command"),
                None,
                Some(sel!(removeCommand:)),
                mtm,
            )
        };
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            key: RefCell::new(key),
            name,
            when,
            run,
            delete,
            profile: RefCell::new(profile),
            view,
            commands,
            edit,
            remove,
            error,
            form: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe {
            this.ivars()
                .name
                .setDelegate(Some(ProtocolObject::from_ref(&*this)));
            this.ivars().when.setTarget(Some(&this));
            this.ivars().run.setTarget(Some(&this));
            this.ivars().delete.setTarget(Some(&this));
            this.ivars()
                .commands
                .setDataSource(Some(ProtocolObject::from_ref(&*this)));
            this.ivars()
                .commands
                .setDelegate(Some(ProtocolObject::from_ref(&*this)));
            this.ivars().commands.setTarget(Some(&this));
            this.ivars()
                .commands
                .setDoubleAction(Some(sel!(editCommand:)));
            this.ivars().edit.setTarget(Some(&this));
            this.ivars().remove.setTarget(Some(&this));
        }
        let wallpaper = FileInput::new(mtm, &this, sel!(wallpaperChanged:));
        wallpaper.set_value(
            &this
                .ivars()
                .profile
                .borrow()
                .wallpaper
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        let heading = ui::heading(mtm, "Configuration");
        let name_label = ui::label(mtm, "Name");
        let when_label = ui::label(mtm, "Run when switching to");
        let metadata = ui::form(
            mtm,
            &[
                [&name_label, &this.ivars().name],
                [&when_label, &this.ivars().when],
            ],
        );
        let back = ui::button(mtm, "Configurations", owner, sel!(showProfiles:));
        let profile_actions = ui::actions(mtm, &[&back, &this.ivars().run, &this.ivars().delete]);
        let title = ui::label(mtm, "Wallpaper");
        let commands_title = ui::heading(mtm, "Commands");
        let add = unsafe {
            NSButton::buttonWithImage_target_action(
                &ui::symbol("plus", "Add command"),
                Some(&this),
                Some(sel!(addCommand:)),
                mtm,
            )
        };
        let actions = ui::stack(mtm, true, &[&add, &this.ivars().edit, &this.ivars().remove]);
        let commands = NSScrollView::new(mtm);
        commands.setHasVerticalScroller(true);
        commands.setDrawsBackground(false);
        commands.setDocumentView(Some(&this.ivars().commands));
        commands
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(120.0)
            .setActive(true);
        let content = ui::stack(
            mtm,
            false,
            &[
                &heading,
                &metadata,
                &profile_actions,
                &title,
                &wallpaper,
                &commands_title,
                &commands,
                &actions,
                &this.ivars().error,
            ],
        );
        for view in [
            &*metadata as &NSView,
            &*profile_actions,
            &*wallpaper,
            &*commands,
            &*this.ivars().error,
        ] {
            view.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        ui::mount(&this.ivars().view, &content, 20.0);
        this.ivars().commands.reloadData();
        this.update_selection();
        this
    }

    pub fn view(&self) -> &NSView {
        &self.ivars().view
    }

    pub fn focus(&self) {
        if let Some(window) = self.ivars().view.window() {
            window.makeFirstResponder(Some(&self.ivars().name));
        }
    }

    fn save(&self, mut profile: Profile) -> bool {
        let Some(owner) = self.ivars().owner.load() else {
            return false;
        };
        profile.name = self.ivars().name.stringValue().to_string();
        profile.when = [Mode::Light, Mode::Dark]
            .into_iter()
            .enumerate()
            .filter_map(|(index, mode)| {
                self.ivars()
                    .when
                    .isSelectedForSegment(index as isize)
                    .then_some(mode)
            })
            .collect();
        let key = self.ivars().key.borrow().clone();
        match owner.save_profile(key.as_deref(), profile.clone()) {
            Ok(()) => {
                *self.ivars().key.borrow_mut() = Some(profile.name.clone());
                self.ivars().run.setEnabled(true);
                self.ivars().delete.setEnabled(true);
                *self.ivars().profile.borrow_mut() = profile;
                self.error("");
                true
            }
            Err(error) => {
                self.error(&error);
                false
            }
        }
    }

    fn error(&self, message: &str) {
        self.ivars()
            .error
            .setStringValue(&NSString::from_str(message));
    }

    fn update_selection(&self) {
        let selected = self.ivars().commands.selectedRow() >= 0;
        self.ivars().edit.setEnabled(selected);
        self.ivars().remove.setEnabled(selected);
        if let Some(form) = self.ivars().form.borrow().as_ref() {
            form.remove.setEnabled(form.table.selectedRow() >= 0);
        }
    }

    fn edit_command(&self, index: Option<usize>) {
        let command = index.map(|index| self.ivars().profile.borrow().commands[index].clone());
        let form = CommandForm::new(self, index, command.as_ref());
        let window = form.window.clone();
        let table = form.table.clone();
        *self.ivars().form.borrow_mut() = Some(form);
        table.reloadData();
        self.update_selection();
        self.ivars()
            .view
            .window()
            .unwrap()
            .beginSheet_completionHandler(&window, None);
    }

    fn finish_editing(&self) {
        let window = self
            .ivars()
            .form
            .borrow()
            .as_ref()
            .map(|form| form.window.clone());
        if let Some(window) = window {
            window.makeFirstResponder(None);
        }
    }

    fn close_command(&self) {
        let form = self.ivars().form.borrow_mut().take();
        if let Some(form) = form {
            self.ivars().view.window().unwrap().endSheet(&form.window);
        }
    }
}

impl CommandForm {
    fn new(editor: &Editor, index: Option<usize>, command: Option<&Command>) -> Self {
        let mtm = editor.mtm();
        let window = ui::window(mtm, "Command", 560.0, 380.0);
        let program = FileInput::new(mtm, editor, sel!(commandChanged:));
        program.set_value(command.map_or("", |command| &command.program));
        let table = ui::table(mtm, "Arguments");
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(editor)));
            table.setDelegate(Some(ProtocolObject::from_ref(editor)));
        }
        let arguments = NSScrollView::new(mtm);
        arguments.setHasVerticalScroller(true);
        arguments.setDrawsBackground(false);
        arguments.setDocumentView(Some(&table));
        arguments
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(150.0)
            .setActive(true);
        let add = unsafe {
            NSButton::buttonWithImage_target_action(
                &ui::symbol("plus", "Add argument"),
                Some(editor),
                Some(sel!(addArgument:)),
                mtm,
            )
        };
        let remove = unsafe {
            NSButton::buttonWithImage_target_action(
                &ui::symbol("minus", "Remove argument"),
                Some(editor),
                Some(sel!(removeArgument:)),
                mtm,
            )
        };
        let edit_actions = ui::stack(mtm, true, &[&add, &remove]);
        let program_label = ui::label(mtm, "Program");
        let args_label = ui::label(mtm, "Arguments");
        let form = ui::form(
            mtm,
            &[[&program_label, &program], [&args_label, &arguments]],
        );
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let save = ui::button(mtm, "Save", editor, sel!(saveCommand:));
        save.setControlSize(NSControlSize::Large);
        save.setTintProminence(NSTintProminence::Primary);
        save.setKeyEquivalent(&NSString::from_str("s"));
        let cancel = ui::button(mtm, "Cancel", editor, sel!(cancelCommand:));
        cancel.setControlSize(NSControlSize::Large);
        cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        let buttons = ui::actions(mtm, &[&cancel, &save]);
        let content = ui::stack(mtm, false, &[&form, &edit_actions, &error, &buttons]);
        for view in [&*form as &NSView, &*error, &*buttons] {
            view.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        ui::mount(&window.contentView().unwrap(), &content, 24.0);
        Self {
            window,
            index,
            program,
            arguments: command
                .map(|command| command.args.clone())
                .unwrap_or_default(),
            table,
            remove,
            error,
        }
    }
}
