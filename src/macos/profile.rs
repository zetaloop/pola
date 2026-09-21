use std::cell::RefCell;

use crate::locale::tr;

use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSIndexSet, NSNotification, NSObject, NSObjectProtocol, NSString,
    ns_string,
};

use super::{Delegate, action, ui};
use crate::{
    config::{Action, Profile},
    mode::Mode,
};

pub struct Ivars {
    owner: Weak<Delegate>,
    key: Option<String>,
    saved: Profile,
    profile: RefCell<Profile>,
    name: Retained<NSTextField>,
    when: Retained<NSSegmentedControl>,
    run: Retained<NSButton>,
    pages: Retained<NSTabViewController>,
    actions: Retained<NSTableView>,
    edit: Retained<NSButton>,
    remove: Retained<NSButton>,
    error: Retained<NSTextField>,
    form: RefCell<Option<Retained<action::Editor>>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Editor;

    unsafe impl NSObjectProtocol for Editor {}
    unsafe impl NSTextFieldDelegate for Editor {}
    unsafe impl NSControlTextEditingDelegate for Editor {
        #[unsafe(method(controlTextDidChange:))]
        fn name_changed(&self, _notification: &NSNotification) { self.update(); }
    }
    unsafe impl NSTableViewDataSource for Editor {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _table: &NSTableView) -> isize {
            self.ivars().profile.borrow().actions.len() as isize
        }

        #[unsafe(method_id(tableView:pasteboardWriterForRow:))]
        fn pasteboard(&self, _table: &NSTableView, row: isize) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
            let item = NSPasteboardItem::new();
            item.setString_forType(&NSString::from_str(&row.to_string()), ns_string!("io.github.zetaloop.pola.action"));
            Some(ProtocolObject::from_retained(item))
        }

        #[unsafe(method(tableView:validateDrop:proposedRow:proposedDropOperation:))]
        fn validate_drop(&self, table: &NSTableView, info: &ProtocolObject<dyn NSDraggingInfo>, row: isize, _operation: NSTableViewDropOperation) -> NSDragOperation {
            let local = info.draggingSource().and_then(|source| source.downcast::<NSTableView>().ok())
                .is_some_and(|source| std::ptr::eq(&*source, table));
            if local && row >= 0 && row as usize <= self.ivars().profile.borrow().actions.len() {
                table.setDropRow_dropOperation(row, NSTableViewDropOperation::Above);
                NSDragOperation::Move
            } else { NSDragOperation::None }
        }

        #[unsafe(method(tableView:acceptDrop:row:dropOperation:))]
        fn accept_drop(&self, _table: &NSTableView, info: &ProtocolObject<dyn NSDraggingInfo>, row: isize, _operation: NSTableViewDropOperation) -> bool {
            let count = self.ivars().profile.borrow().actions.len();
            if let Some(index) = info.draggingPasteboard().stringForType(ns_string!("io.github.zetaloop.pola.action"))
                .and_then(|value| value.to_string().parse::<usize>().ok())
                && row >= 0 && row as usize <= count && index < count
            {
                let target = row as usize - usize::from(index < row as usize);
                {
                    let mut profile = self.ivars().profile.borrow_mut();
                    let action = profile.actions.remove(index);
                    profile.actions.insert(target, action);
                }
                self.reload(Some(target));
                true
            } else { false }
        }
    }
    unsafe impl NSTableViewDelegate for Editor {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
            self.ivars().profile.borrow().actions.get(row as usize)
                .map(|action| ui::cell(self.mtm(), &action.summary(), None).into_super())
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, _notification: &NSNotification) { self.update(); }
    }
    impl Editor {
        #[unsafe(method(whenChanged:))]
        fn when_changed(&self, _sender: &NSObject) { self.update(); }

        #[unsafe(method(runProfile:))]
        fn run_profile(&self, _sender: &NSObject) {
            if let Some(owner) = self.ivars().owner.load() && let Some(name) = &self.ivars().key {
                self.error(&owner.run_profile(name).err().unwrap_or_default());
            }
        }

        #[unsafe(method(saveProfile:))]
        fn save_profile(&self, _sender: &NSObject) {
            self.finish_editing();
            if let Some(owner) = self.ivars().owner.load() {
                match owner.save_profile(self.ivars().key.as_deref(), self.current()) {
                    Ok(()) => owner.show_profiles(),
                    Err(error) => self.error(&error),
                }
            }
        }

        #[unsafe(method(deleteProfile:))]
        fn delete_profile(&self, _sender: &NSObject) {
            if let Some(owner) = self.ivars().owner.load() && let Some(name) = &self.ivars().key {
                let mut config = owner.config();
                config.profiles.retain(|profile| &profile.name != name);
                match owner.save_config(config) {
                    Ok(()) => owner.show_profiles(),
                    Err(error) => self.error(&error),
                }
            }
        }

        #[unsafe(method(addAction:))]
        fn add_action(&self, _sender: &NSObject) { self.open_action(None); }

        #[unsafe(method(editAction:))]
        fn edit_action(&self, _sender: &NSObject) {
            let row = self.ivars().actions.selectedRow();
            if row >= 0 { self.open_action(Some(row as usize)); }
        }

        #[unsafe(method(removeAction:))]
        fn remove_action(&self, _sender: &NSObject) {
            let row = self.ivars().actions.selectedRow();
            if row >= 0 {
                self.ivars().profile.borrow_mut().actions.remove(row as usize);
                let count = self.ivars().profile.borrow().actions.len();
                self.reload((count > 0).then(|| (row as usize).min(count - 1)));
            }
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
        let name = NSTextField::textFieldWithString(&NSString::from_str(&profile.name), mtm);
        let when = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_slice(&[
                    &*NSString::from_str(tr!("Light")),
                    &*NSString::from_str(tr!("Dark")),
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
                &NSString::from_str(tr!("Run")),
                None,
                Some(sel!(runProfile:)),
                mtm,
            )
        };
        let edit = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(tr!("Edit")),
                None,
                Some(sel!(editAction:)),
                mtm,
            )
        };
        let remove = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(tr!("Remove")),
                None,
                Some(sel!(removeAction:)),
                mtm,
            )
        };
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            key,
            saved: profile.clone(),
            profile: RefCell::new(profile),
            name,
            when,
            run,
            pages: ui::pages(mtm, &[]),
            actions: ui::table(mtm, tr!("Actions")),
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
            this.ivars().edit.setTarget(Some(&this));
            this.ivars().remove.setTarget(Some(&this));
            this.ivars()
                .actions
                .setDataSource(Some(ProtocolObject::from_ref(&*this)));
            this.ivars()
                .actions
                .setDelegate(Some(ProtocolObject::from_ref(&*this)));
            this.ivars().actions.setTarget(Some(&this));
            this.ivars()
                .actions
                .setDoubleAction(Some(sel!(editAction:)));
        }
        this.ivars()
            .actions
            .registerForDraggedTypes(&NSArray::from_slice(&[ns_string!(
                "io.github.zetaloop.pola.action"
            )]));
        this.ivars()
            .actions
            .setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
        let heading = ui::heading(mtm, tr!("Configuration"));
        let name_label = ui::label(mtm, tr!("Name"));
        let when_label = ui::label(mtm, tr!("Run when switching to"));
        let metadata = ui::form(
            mtm,
            &[
                [&name_label, &this.ivars().name],
                [&when_label, &this.ivars().when],
            ],
        );
        let title = ui::heading(mtm, tr!("Actions"));
        let list = NSScrollView::new(mtm);
        list.setHasVerticalScroller(true);
        list.setDrawsBackground(false);
        list.setDocumentView(Some(&this.ivars().actions));
        list.heightAnchor()
            .constraintGreaterThanOrEqualToConstant(180.0)
            .setActive(true);
        let add = ui::button(mtm, tr!("Add action"), &this, sel!(addAction:));
        let controls = ui::stack(mtm, true, &[&add, &this.ivars().edit, &this.ivars().remove]);
        let save = ui::button(mtm, tr!("Save"), &this, sel!(saveProfile:));
        save.setTintProminence(NSTintProminence::Primary);
        let cancel = ui::button(mtm, tr!("Cancel"), owner, sel!(showProfiles:));
        let delete = ui::button(
            mtm,
            tr!("Delete configuration"),
            &this,
            sel!(deleteProfile:),
        );
        delete.setEnabled(this.ivars().key.is_some());
        let buttons = ui::actions(mtm, &[&this.ivars().run, &delete, &cancel, &save]);
        let content = ui::stack(
            mtm,
            false,
            &[
                &heading,
                &metadata,
                &title,
                &list,
                &controls,
                &this.ivars().error,
                &buttons,
            ],
        );
        for view in [
            &*metadata as &NSView,
            &*list,
            &*this.ivars().error,
            &*buttons,
        ] {
            view.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        let view = NSView::new(mtm);
        ui::scroll(&view, &content);
        let controller = NSViewController::new(mtm);
        controller.setView(&view);
        this.ivars()
            .pages
            .addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(&controller));
        this.reload(None);
        this
    }

    pub fn controller(&self) -> &NSTabViewController {
        &self.ivars().pages
    }

    pub fn focus(&self) {
        if let Some(window) = self.ivars().pages.view().window() {
            window.makeFirstResponder(Some(&self.ivars().name));
        }
    }

    pub fn update(&self) {
        let selected = self.ivars().actions.selectedRow() >= 0;
        self.ivars().edit.setEnabled(selected);
        self.ivars().remove.setEnabled(selected);
        let idle = self
            .ivars()
            .owner
            .load()
            .is_some_and(|owner| !owner.ivars().client.state().busy);
        self.ivars()
            .run
            .setEnabled(idle && self.ivars().key.is_some() && self.current() == self.ivars().saved);
    }

    fn current(&self) -> Profile {
        let mut profile = self.ivars().profile.borrow().clone();
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
        profile
    }

    pub fn save_action(&self, index: Option<usize>, action: Action) -> Result<(), String> {
        action.validate().map_err(|error| error.to_string())?;
        {
            let mut profile = self.ivars().profile.borrow_mut();
            match index {
                Some(index) => profile.actions[index] = action,
                None => profile.actions.push(action),
            }
        }
        self.reload(index.or_else(|| self.ivars().profile.borrow().actions.len().checked_sub(1)));
        Ok(())
    }

    fn open_action(&self, index: Option<usize>) {
        self.finish_editing();
        let action = index
            .map(|index| self.ivars().profile.borrow().actions[index].clone())
            .unwrap_or(Action::Color { mode: Mode::Light });
        let form = action::Editor::new(self.mtm(), self, index, &action);
        let controller = NSViewController::new(self.mtm());
        controller.setView(form.view());
        self.ivars()
            .pages
            .addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(&controller));
        self.ivars().pages.setSelectedTabViewItemIndex(1);
        *self.ivars().form.borrow_mut() = Some(form);
    }

    pub fn close_action(&self) {
        self.finish_editing();
        self.ivars().pages.setSelectedTabViewItemIndex(0);
        let items = self.ivars().pages.tabViewItems();
        if items.len() > 1 {
            self.ivars()
                .pages
                .removeTabViewItem(&items.objectAtIndex(1));
        }
    }

    fn finish_editing(&self) {
        if let Some(window) = self.ivars().pages.view().window() {
            window.makeFirstResponder(None);
        }
    }

    fn reload(&self, row: Option<usize>) {
        self.ivars().actions.reloadData();
        if let Some(row) = row {
            self.ivars()
                .actions
                .selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
        }
        self.update();
    }

    fn error(&self, message: &str) {
        self.ivars()
            .error
            .setStringValue(&NSString::from_str(message));
    }
}
