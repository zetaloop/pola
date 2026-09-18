use std::{cell::RefCell, path::PathBuf};

use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    sel,
};
use objc2_app_kit::{
    NSButton, NSColor, NSImage, NSStackView, NSStackViewDistribution, NSTextField, NSTextView,
    NSWindow,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};

use crate::{
    config::{Command, Profile},
    mode::Mode,
};

use super::{Delegate, file::FileInput, ui};

struct Argument {
    row: Retained<NSStackView>,
    field: Retained<NSTextView>,
    remove: Retained<NSButton>,
}

struct CommandForm {
    window: Retained<NSWindow>,
    index: Option<usize>,
    program: Retained<FileInput>,
    list: Retained<NSStackView>,
    arguments: Vec<Argument>,
    error: Retained<NSTextField>,
}

pub struct Ivars {
    owner: Weak<Delegate>,
    mode: Mode,
    profile: RefCell<Profile>,
    view: Retained<objc2_app_kit::NSView>,
    commands: Retained<NSStackView>,
    error: Retained<NSTextField>,
    form: RefCell<Option<CommandForm>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Editor;

    unsafe impl NSObjectProtocol for Editor {}

    impl Editor {
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
        fn edit(&self, sender: &NSButton) {
            self.edit_command(Some(sender.tag() as usize));
        }

        #[unsafe(method(removeCommand:))]
        fn remove_command(&self, sender: &NSButton) {
            let mut profile = self.ivars().profile.borrow().clone();
            profile.commands.remove(sender.tag() as usize);
            if self.save(profile) {
                self.update_commands();
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
            if let Some(form) = self.ivars().form.borrow_mut().as_mut() {
                form.add_argument(self, "");
            }
        }

        #[unsafe(method(removeArgument:))]
        fn remove_argument(&self, sender: &NSButton) {
            if let Some(form) = self.ivars().form.borrow_mut().as_mut() {
                let argument = form.arguments.remove(sender.tag() as usize);
                form.list.removeArrangedSubview(&argument.row);
                argument.row.removeFromSuperview();
                for (index, argument) in form.arguments.iter().enumerate() {
                    argument.remove.setTag(index as isize);
                }
            }
        }

        #[unsafe(method(saveCommand:))]
        fn save_command(&self, _sender: &NSObject) {
            let result = {
                let form = self.ivars().form.borrow();
                let Some(form) = form.as_ref() else {
                    return;
                };
                form.window.makeFirstResponder(None);
                let program = form.program.value();
                if program.is_empty() {
                    form.error
                        .setStringValue(&NSString::from_str("Enter a program to run."));
                    return;
                }
                let command = Command {
                    program,
                    args: form
                        .arguments
                        .iter()
                        .map(|arg| arg.field.string().to_string())
                        .collect(),
                };
                let mut profile = self.ivars().profile.borrow().clone();
                match form.index {
                    Some(index) => profile.commands[index] = command,
                    None => profile.commands.push(command),
                }
                profile
            };
            if self.save(result) {
                self.close_command();
                self.update_commands();
            } else if let Some(form) = self.ivars().form.borrow().as_ref() {
                form.error.setStringValue(&self.ivars().error.stringValue());
            }
        }

        #[unsafe(method(cancelCommand:))]
        fn cancel_command(&self, _sender: &NSObject) {
            self.close_command();
        }

        #[unsafe(method(closeInspector:))]
        fn close(&self, _sender: &NSObject) {
            if let Some(window) = self.ivars().view.window() {
                window.makeFirstResponder(None);
            }
            if let Some(owner) = self.ivars().owner.load() {
                owner.close_inspector();
            }
        }
    }
);

impl Editor {
    pub fn new(
        mtm: MainThreadMarker,
        owner: &Delegate,
        mode: Mode,
        profile: Profile,
    ) -> Retained<Self> {
        let view = objc2_app_kit::NSView::new(mtm);
        let commands = ui::stack(mtm, false, &[]);
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            mode,
            profile: RefCell::new(profile),
            view,
            commands,
            error,
            form: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
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
        let name = ui::label(mtm, &format!("{mode} appearance"), 17.0);
        let title = ui::label(mtm, "Wallpaper", 13.0);
        let commands_title = ui::label(mtm, "Commands", 17.0);
        let add = ui::button(mtm, "Add…", &this, sel!(addCommand:));
        let heading = ui::stack(mtm, true, &[&commands_title, &add]);
        heading.setDistribution(NSStackViewDistribution::EqualSpacing);
        let commands = ui::scroll(mtm, &this.ivars().commands, 150.0);
        let done = ui::button(mtm, "Close", &this, sel!(closeInspector:));
        let content = ui::stack(
            mtm,
            false,
            &[
                &name,
                &title,
                &wallpaper,
                &heading,
                &commands,
                &this.ivars().error,
                &done,
            ],
        );
        for view in [
            &*wallpaper as &objc2_app_kit::NSView,
            &*heading,
            &*commands,
            &*this.ivars().error,
        ] {
            view.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        ui::mount(&this.ivars().view, &content, 20.0);
        this.update_commands();
        this
    }

    pub fn view(&self) -> &objc2_app_kit::NSView {
        &self.ivars().view
    }

    fn save(&self, profile: Profile) -> bool {
        let Some(owner) = self.ivars().owner.load() else {
            return false;
        };
        match owner.save_profile(self.ivars().mode, profile.clone()) {
            Ok(()) => {
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

    fn update_commands(&self) {
        let list = &self.ivars().commands;
        for child in list.arrangedSubviews() {
            list.removeArrangedSubview(&child);
            child.removeFromSuperview();
        }
        let profile = self.ivars().profile.borrow();
        if profile.commands.is_empty() {
            let empty = ui::label(
                self.mtm(),
                "Run programs when this appearance becomes active.",
                13.0,
            );
            empty.setTextColor(Some(&NSColor::secondaryLabelColor()));
            list.addArrangedSubview(&empty);
        }
        for (index, command) in profile.commands.iter().enumerate() {
            let name = std::path::Path::new(&command.program)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| command.program.clone());
            let edit = ui::button(self.mtm(), &name, self, sel!(editCommand:));
            edit.setTag(index as isize);
            edit.setToolTip(Some(&NSString::from_str(&command.program)));
            let remove = ui::button(self.mtm(), "Remove", self, sel!(removeCommand:));
            remove.setTag(index as isize);
            let row = ui::stack(self.mtm(), true, &[&edit, &remove]);
            row.setDistribution(NSStackViewDistribution::EqualSpacing);
            list.addArrangedSubview(&row);
            row.widthAnchor()
                .constraintEqualToAnchor(&list.widthAnchor())
                .setActive(true);
        }
    }

    fn edit_command(&self, index: Option<usize>) {
        let command = index.map(|index| self.ivars().profile.borrow().commands[index].clone());
        let mut form = CommandForm::new(self, index, command.as_ref());
        if let Some(command) = command {
            for arg in command.args {
                form.add_argument(self, &arg);
            }
        }
        self.ivars()
            .view
            .window()
            .unwrap()
            .beginSheet_completionHandler(&form.window, None);
        *self.ivars().form.borrow_mut() = Some(form);
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
        let window = ui::window(mtm, "Command", 520.0, 400.0);
        let program = FileInput::new(mtm, editor, sel!(commandChanged:));
        program.set_value(command.map_or("", |command| &command.program));
        let list = ui::stack(mtm, false, &[]);
        let arguments = ui::scroll(mtm, &list, 170.0);
        let program_label = ui::label(mtm, "Program", 15.0);
        let args_label = ui::label(mtm, "Arguments", 15.0);
        let add = ui::button(mtm, "Add argument", editor, sel!(addArgument:));
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let save = ui::button(mtm, "Save", editor, sel!(saveCommand:));
        let cancel = ui::button(mtm, "Cancel", editor, sel!(cancelCommand:));
        cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        let buttons = ui::stack(mtm, true, &[&cancel, &save]);
        let content = ui::stack(
            mtm,
            false,
            &[
                &program_label,
                &program,
                &args_label,
                &arguments,
                &add,
                &error,
                &buttons,
            ],
        );
        for view in [&*program as &objc2_app_kit::NSView, &*arguments, &*error] {
            view.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        ui::mount(&window.contentView().unwrap(), &content, 24.0);
        Self {
            window,
            index,
            program,
            list,
            arguments: Vec::new(),
            error,
        }
    }

    fn add_argument(&mut self, editor: &Editor, text: &str) {
        let scroll = NSTextView::scrollableTextView(editor.mtm());
        let field = scroll
            .documentView()
            .unwrap()
            .downcast::<NSTextView>()
            .unwrap();
        field.setRichText(false);
        field.setAutomaticQuoteSubstitutionEnabled(false);
        field.setAutomaticDashSubstitutionEnabled(false);
        field.setAutomaticTextReplacementEnabled(false);
        field.setAutomaticSpellingCorrectionEnabled(false);
        field.setSmartInsertDeleteEnabled(false);
        field.setString(&NSString::from_str(text));
        scroll
            .heightAnchor()
            .constraintEqualToConstant(54.0)
            .setActive(true);
        let remove = ui::button(editor.mtm(), "Remove", editor, sel!(removeArgument:));
        remove.setTag(self.arguments.len() as isize);
        let row = ui::stack(editor.mtm(), true, &[&scroll, &remove]);
        scroll.setContentHuggingPriority_forOrientation(
            1.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        self.list.addArrangedSubview(&row);
        row.widthAnchor()
            .constraintEqualToAnchor(&self.list.widthAnchor())
            .setActive(true);
        self.arguments.push(Argument { row, field, remove });
    }
}
