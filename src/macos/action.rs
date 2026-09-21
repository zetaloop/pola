use std::cell::RefCell;

use crate::locale::tr;

use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSRect, NSString};

use super::{file::FileInput, profile, ui};
use crate::{
    config::{Action, Command},
    mode::Mode,
};

struct Argument {
    field: Retained<NSTextField>,
    remove: Retained<NSButton>,
    view: Retained<NSStackView>,
}

enum Fields {
    Color(Retained<NSSegmentedControl>),
    Wallpaper {
        path: Retained<FileInput>,
        preview: Retained<NSImageView>,
    },
    Command {
        program: Retained<FileInput>,
        arguments: Retained<NSStackView>,
        rows: Vec<Argument>,
        wait: Retained<NSButton>,
    },
}

pub struct Ivars {
    owner: Weak<profile::Editor>,
    index: Option<usize>,
    view: Retained<NSView>,
    kind: Retained<NSPopUpButton>,
    body: Retained<NSStackView>,
    error: Retained<NSTextField>,
    fields: RefCell<Option<Fields>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Editor;

    unsafe impl NSObjectProtocol for Editor {}
    impl Editor {
        #[unsafe(method(kindChanged:))]
        fn kind_changed(&self, sender: &NSPopUpButton) {
            if let Some(action) = Action::choices().get(sender.indexOfSelectedItem() as usize) {
                self.build(action);
            }
        }

        #[unsafe(method(pathChanged:))]
        fn path_changed(&self, _sender: &NSObject) { self.preview(); }

        #[unsafe(method(addArgument:))]
        fn add_argument(&self, _sender: &NSObject) {
            let row = {
                let mut fields = self.ivars().fields.borrow_mut();
                let Some(Fields::Command { rows, arguments, .. }) = fields.as_mut() else { return };
                let argument = self.argument("", rows.len(), arguments);
                let field = argument.field.clone();
                rows.push(argument);
                field
            };
            self.layout_arguments();
            if let Some(window) = self.ivars().view.window() { window.makeFirstResponder(Some(&row)); }
        }

        #[unsafe(method(removeArgument:))]
        fn remove_argument(&self, sender: &NSButton) {
            if let Some(window) = self.ivars().view.window() { window.makeFirstResponder(None); }
            let index = sender.tag() as usize;
            let focus = {
                let mut fields = self.ivars().fields.borrow_mut();
                let Some(Fields::Command { rows, .. }) = fields.as_mut() else { return };
                if index >= rows.len() { return; }
                rows.remove(index);
                rows.get(index.min(rows.len().saturating_sub(1))).map(|row| row.field.clone())
            };
            self.layout_arguments();
            if let Some(field) = focus && let Some(window) = self.ivars().view.window() {
                window.makeFirstResponder(Some(&field));
            }
        }

        #[unsafe(method(saveAction:))]
        fn save_action(&self, _sender: &NSObject) {
            if let Some(window) = self.ivars().view.window() { window.makeFirstResponder(None); }
            let action = match self.ivars().fields.borrow().as_ref().unwrap() {
                Fields::Color(control) => Action::Color { mode: if control.selectedSegment() == 0 { Mode::Light } else { Mode::Dark } },
                Fields::Wallpaper { path, .. } => Action::Wallpaper { path: path.value().into() },
                Fields::Command { program, rows, wait, .. } => Action::Command(Command {
                    program: program.value(), args: rows.iter().map(|row| row.field.stringValue().to_string()).collect(),
                    wait: wait.state() == NSControlStateValueOn,
                }),
            };
            let result = action.validate().map_err(|error| error.to_string()).and_then(|()| {
                self.ivars().owner.load().ok_or(tr!("The configuration editor has closed.").to_owned())?
                    .save_action(self.ivars().index, action)
            });
            match result {
                Ok(()) => { if let Some(owner) = self.ivars().owner.load() { owner.close_action(); } }
                Err(error) => self.ivars().error.setStringValue(&NSString::from_str(&error)),
            }
        }

        #[unsafe(method(cancelAction:))]
        fn cancel_action(&self, _sender: &NSObject) {
            if let Some(owner) = self.ivars().owner.load() { owner.close_action(); }
        }
    }
);

impl Editor {
    pub fn new(
        mtm: MainThreadMarker,
        owner: &profile::Editor,
        index: Option<usize>,
        action: &Action,
    ) -> Retained<Self> {
        let kind =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
        let choices = Action::choices();
        kind.addItemsWithTitles(&NSArray::from_retained_slice(
            &choices
                .iter()
                .map(|action| NSString::from_str(action.title()))
                .collect::<Vec<_>>(),
        ));
        kind.selectItemAtIndex(
            choices
                .iter()
                .position(|choice| std::mem::discriminant(choice) == std::mem::discriminant(action))
                .unwrap() as isize,
        );
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            index,
            view: NSView::new(mtm),
            kind,
            body: ui::stack(mtm, false, &[]),
            error,
            fields: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe {
            this.ivars().kind.setTarget(Some(&this));
            this.ivars().kind.setAction(Some(sel!(kindChanged:)));
        }
        let heading = ui::heading(mtm, tr!("Action"));
        let save = ui::button(mtm, tr!("Save"), &this, sel!(saveAction:));
        save.setTintProminence(NSTintProminence::Primary);
        let cancel = ui::button(mtm, tr!("Cancel"), &this, sel!(cancelAction:));
        let buttons = ui::actions(mtm, &[&cancel, &save]);
        let content = ui::stack(
            mtm,
            false,
            &[
                &heading,
                &this.ivars().kind,
                &this.ivars().body,
                &this.ivars().error,
                &buttons,
            ],
        );
        for view in [
            &*this.ivars().body as &NSView,
            &*this.ivars().error,
            &*buttons,
        ] {
            view.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        ui::scroll(&this.ivars().view, &content);
        this.build(action);
        this
    }

    pub fn view(&self) -> &NSView {
        &self.ivars().view
    }

    fn build(&self, action: &Action) {
        if let Some(window) = self.ivars().view.window() {
            window.makeFirstResponder(None);
        }
        let mtm = self.mtm();
        let mut views: Vec<Retained<NSView>> = Vec::new();
        let fields = match action {
            Action::Color { mode } => {
                let control = unsafe {
                    NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                        &NSArray::from_retained_slice(&[
                            NSString::from_str(tr!("Light")),
                            NSString::from_str(tr!("Dark")),
                        ]),
                        NSSegmentSwitchTracking::SelectOne,
                        None,
                        None,
                        mtm,
                    )
                };
                control.setSelectedSegment(isize::from(*mode == Mode::Dark));
                views.push(control.clone().into_super().into_super());
                Fields::Color(control)
            }
            Action::Wallpaper { path } => {
                let input = FileInput::new(mtm, self, sel!(pathChanged:));
                input.set_value(&path.to_string_lossy());
                let preview = NSImageView::new(mtm);
                preview.setImageScaling(NSImageScaling::ScaleProportionallyDown);
                preview
                    .heightAnchor()
                    .constraintEqualToConstant(180.0)
                    .setActive(true);
                views.push(preview.clone().into_super().into_super());
                views.push(input.clone().into_super().into_super().into_super());
                Fields::Wallpaper {
                    path: input,
                    preview,
                }
            }
            Action::Command(command) => {
                let program = FileInput::new(mtm, self, sel!(pathChanged:));
                program.set_value(&command.program);
                let arguments = ui::stack(mtm, false, &[]);
                let rows = command
                    .args
                    .iter()
                    .enumerate()
                    .map(|(index, value)| self.argument(value, index, &arguments))
                    .collect();
                let add = ui::button(mtm, tr!("Add argument"), self, sel!(addArgument:));
                let wait = unsafe {
                    NSButton::checkboxWithTitle_target_action(
                        &NSString::from_str(tr!("Continue after the program exits")),
                        None,
                        None,
                        mtm,
                    )
                };
                wait.setState(if command.wait {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                views.push(ui::label(mtm, tr!("Program")).into_super().into_super());
                views.push(program.clone().into_super().into_super().into_super());
                views.push(arguments.clone().into_super());
                views.push(add.into_super().into_super());
                views.push(wait.clone().into_super().into_super());
                Fields::Command {
                    program,
                    arguments,
                    rows,
                    wait,
                }
            }
        };
        *self.ivars().fields.borrow_mut() = Some(fields);
        self.ivars().body.setViews_inGravity(
            &NSArray::from_retained_slice(&views),
            NSStackViewGravity::Top,
        );
        for view in &views {
            view.widthAnchor()
                .constraintEqualToAnchor(&self.ivars().body.widthAnchor())
                .setActive(true);
        }
        self.ivars().error.setStringValue(&NSString::new());
        self.layout_arguments();
        self.preview();
    }

    fn preview(&self) {
        if let Some(Fields::Wallpaper { path, preview }) = self.ivars().fields.borrow().as_ref() {
            let image = NSImage::initWithContentsOfFile(
                NSImage::alloc(),
                &NSString::from_str(&path.value()),
            );
            preview.setImage(image.as_deref());
            preview.setHidden(image.is_none());
            self.ivars().error.setStringValue(&NSString::from_str(
                if image.is_none() && !path.value().is_empty() {
                    tr!("Wallpaper preview unavailable")
                } else {
                    ""
                },
            ));
        }
    }

    fn argument(&self, value: &str, index: usize, parent: &NSStackView) -> Argument {
        let field = NSTextField::textFieldWithString(&NSString::from_str(value), self.mtm());
        field.setPlaceholderString(Some(&NSString::from_str(&tr!(
            "Argument {number}",
            number = index + 1
        ))));
        if let Some(cell) = field.cell() {
            cell.setUsesSingleLineMode(false);
        }
        let remove = ui::button(self.mtm(), tr!("Remove"), self, sel!(removeArgument:));
        remove.setTag(index as isize);
        let view = ui::stack(self.mtm(), true, &[&field, &remove]);
        remove.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityDefaultHigh,
            NSLayoutConstraintOrientation::Horizontal,
        );
        parent.addView_inGravity(&view, NSStackViewGravity::Top);
        view.widthAnchor()
            .constraintEqualToAnchor(&parent.widthAnchor())
            .setActive(true);
        Argument {
            field,
            remove,
            view,
        }
    }

    fn layout_arguments(&self) {
        let layout = {
            let fields = self.ivars().fields.borrow();
            let Some(Fields::Command {
                arguments, rows, ..
            }) = fields.as_ref()
            else {
                return;
            };
            for (index, row) in rows.iter().enumerate() {
                row.remove.setTag(index as isize);
                row.field
                    .setPlaceholderString(Some(&NSString::from_str(&tr!(
                        "Argument {number}",
                        number = index + 1
                    ))));
            }
            (
                arguments.clone(),
                rows.iter()
                    .map(|row| row.view.clone().into_super())
                    .collect::<Vec<_>>(),
            )
        };
        layout.0.setViews_inGravity(
            &NSArray::from_retained_slice(&layout.1),
            NSStackViewGravity::Top,
        );
    }
}
