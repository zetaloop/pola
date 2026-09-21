use std::cell::RefCell;

use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSIndexSet, NSNotification, NSObject, NSObjectProtocol, NSRect,
    NSString, ns_string,
};

use super::{Delegate, action, ui};
use crate::{
    config::{Action, Profile},
    locale::tr,
    mode::Mode,
};

pub struct Ivars {
    owner: Weak<Delegate>,
    key: RefCell<Option<String>>,
    profile: RefCell<Profile>,
    name: Retained<NSTextField>,
    when: Retained<NSSegmentedControl>,
    create: Retained<NSButton>,
    run: Retained<NSButton>,
    body: Retained<NSStackView>,
    pages: Retained<NSTabViewController>,
    actions: Retained<NSTableView>,
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

        #[unsafe(method(controlTextDidEndEditing:))]
        fn name_edited(&self, _notification: &NSNotification) {
            if self.ivars().key.borrow().is_some() {
                let mut profile = self.ivars().profile.borrow().clone();
                profile.name = self.ivars().name.stringValue().to_string();
                self.error(&self.persist(profile).err().unwrap_or_default());
            }
        }
    }
    unsafe impl NSTableViewDataSource for Editor {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _table: &NSTableView) -> isize { self.ivars().profile.borrow().actions.len() as isize }

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
            let mut profile = self.ivars().profile.borrow().clone();
            if let Some(index) = info.draggingPasteboard().stringForType(ns_string!("io.github.zetaloop.pola.action"))
                .and_then(|value| value.to_string().parse::<usize>().ok())
                && row >= 0 && row as usize <= profile.actions.len() && index < profile.actions.len()
            {
                let target = row as usize - usize::from(index < row as usize);
                let action = profile.actions.remove(index);
                profile.actions.insert(target, action);
                match self.persist(profile) {
                    Ok(()) => { self.reload(Some(target)); self.error(""); true }
                    Err(error) => { self.error(&error); false }
                }
            } else { false }
        }
    }
    unsafe impl NSTableViewDelegate for Editor {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
            self.ivars().profile.borrow().actions.get(row as usize).map(|action| {
                let cell = ui::cell(self.mtm(), &action.summary(), None);
                cell.setToolTip(Some(&NSString::from_str(&action.summary())));
                cell.into_super()
            })
        }
    }
    impl Editor {
        #[unsafe(method(whenChanged:))]
        fn when_changed(&self, _sender: &NSObject) {
            let mut profile = self.ivars().profile.borrow().clone();
            profile.when = [Mode::Light, Mode::Dark].into_iter().enumerate()
                .filter_map(|(index, mode)| self.ivars().when.isSelectedForSegment(index as isize).then_some(mode)).collect();
            self.error(&self.persist(profile).err().unwrap_or_default());
        }

        #[unsafe(method(runProfile:))]
        fn run_profile(&self, _sender: &NSObject) {
            let key = self.ivars().key.borrow().clone();
            if let Some(owner) = self.ivars().owner.load() && let Some(name) = key {
                self.error(&owner.run_profile(&name).err().unwrap_or_default());
            }
        }

        #[unsafe(method(createProfile:))]
        fn create_profile(&self, _sender: &NSObject) {
            self.finish_editing();
            let profile = Profile { name: self.ivars().name.stringValue().to_string(), ..Profile::default() };
            self.error(&self.persist(profile).err().unwrap_or_default());
        }

        #[unsafe(method(deleteProfile:))]
        fn delete_profile(&self, _sender: &NSObject) {
            let key = self.ivars().key.borrow().clone();
            if let Some(owner) = self.ivars().owner.load() && let Some(name) = key {
                let mut config = owner.config();
                config.profiles.retain(|profile| profile.name != name);
                match owner.save_config(config) {
                    Ok(()) => owner.show_profiles(),
                    Err(error) => self.error(&error),
                }
            }
        }

        #[unsafe(method(addAction:))]
        fn add_action(&self, sender: &NSPopUpButton) {
            if let Some(index) = sender.indexOfSelectedItem().checked_sub(1)
                && let Some(action) = Action::choices().get(index as usize)
            { self.open_action(None, action.clone()); }
        }

        #[unsafe(method(editAction:))]
        fn edit_action(&self, _sender: &NSObject) {
            let index = self.action_row();
            if index >= 0 {
                let action = self.ivars().profile.borrow().actions[index as usize].clone();
                self.open_action(Some(index as usize), action);
            }
        }

        #[unsafe(method(removeAction:))]
        fn remove_action(&self, _sender: &NSObject) {
            let index = self.action_row();
            if index >= 0 {
                let mut profile = self.ivars().profile.borrow().clone();
                profile.actions.remove(index as usize);
                match self.persist(profile) {
                    Ok(()) => { self.reload(None); self.error(""); }
                    Err(error) => self.error(&error),
                }
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
        name.setPlaceholderString(Some(&NSString::from_str(tr!("Name"))));
        let when = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&[
                    NSString::from_str(tr!("Light")),
                    NSString::from_str(tr!("Dark")),
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
        let create = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(tr!("Create configuration")),
                None,
                Some(sel!(createProfile:)),
                mtm,
            )
        };
        create.setTintProminence(NSTintProminence::Primary);
        let run = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(tr!("Run")),
                None,
                Some(sel!(runProfile:)),
                mtm,
            )
        };
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            key: RefCell::new(key),
            profile: RefCell::new(profile),
            name,
            when,
            create,
            run,
            body: ui::stack(mtm, false, &[]),
            pages: ui::pages(mtm, &[]),
            actions: ui::table(mtm, tr!("Actions")),
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
            this.ivars().create.setTarget(Some(&this));
            this.ivars()
                .actions
                .setDataSource(Some(ProtocolObject::from_ref(&*this)));
            this.ivars()
                .actions
                .setDelegate(Some(ProtocolObject::from_ref(&*this)));
            this.ivars().actions.setTarget(Some(&this));
            this.ivars().actions.setAction(Some(sel!(editAction:)));
        }
        this.ivars()
            .actions
            .registerForDraggedTypes(&NSArray::from_slice(&[ns_string!(
                "io.github.zetaloop.pola.action"
            )]));
        this.ivars()
            .actions
            .setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
        let menu = NSMenu::new(mtm);
        let remove = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str(tr!("Remove")),
                Some(sel!(removeAction:)),
                &NSString::new(),
            )
        };
        unsafe {
            remove.setTarget(Some(&this));
            this.ivars().actions.setMenu(Some(&menu));
        }

        let back = ui::button(mtm, tr!("Configurations"), owner, sel!(showProfiles:));
        back.setImage(Some(&ui::symbol("chevron.backward", tr!("Configurations"))));
        let name_label = ui::label(mtm, tr!("Name"));
        let name = ui::form(mtm, &[[&name_label, &this.ivars().name]]);
        let when_label = ui::label(mtm, tr!("Run when switching to"));
        let when = ui::form(mtm, &[[&when_label, &this.ivars().when]]);
        let title = ui::heading(mtm, tr!("Actions"));
        let add =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, true);
        add.addItemWithTitle(&NSString::from_str(tr!("Add action")));
        for action in Action::choices() {
            add.addItemWithTitle(&NSString::from_str(action.title()));
        }
        unsafe {
            add.setTarget(Some(&this));
            add.setAction(Some(sel!(addAction:)));
        }
        let header = ui::stack(mtm, true, &[&title, &add]);
        header.setDistribution(NSStackViewDistribution::EqualSpacing);
        let list = NSScrollView::new(mtm);
        list.setHasVerticalScroller(true);
        list.setDrawsBackground(false);
        list.setDocumentView(Some(&this.ivars().actions));
        list.heightAnchor()
            .constraintGreaterThanOrEqualToConstant(180.0)
            .setActive(true);
        let delete = ui::button(
            mtm,
            tr!("Delete configuration"),
            &this,
            sel!(deleteProfile:),
        );
        let buttons = ui::actions(mtm, &[&delete, &this.ivars().run]);
        this.ivars().body.setViews_inGravity(
            &NSArray::from_slice(&[&*when as &NSView, &header, &list, &buttons]),
            NSStackViewGravity::Top,
        );
        for view in [&*when as &NSView, &*header, &*list, &*buttons] {
            view.widthAnchor()
                .constraintEqualToAnchor(&this.ivars().body.widthAnchor())
                .setActive(true);
        }
        let content = ui::stack(
            mtm,
            false,
            &[
                &back,
                &name,
                &this.ivars().create,
                &this.ivars().body,
                &this.ivars().error,
            ],
        );
        for view in [&*name as &NSView, &*this.ivars().body, &*this.ivars().error] {
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
        let exists = self.ivars().key.borrow().is_some();
        self.ivars().create.setHidden(exists);
        self.ivars().create.setEnabled(
            !self
                .ivars()
                .name
                .stringValue()
                .to_string()
                .trim()
                .is_empty(),
        );
        self.ivars().body.setHidden(!exists);
        let idle = self
            .ivars()
            .owner
            .load()
            .is_some_and(|owner| !owner.ivars().client.state().busy);
        self.ivars().run.setEnabled(
            idle && self.ivars().name.stringValue().to_string()
                == self.ivars().profile.borrow().name,
        );
    }

    fn persist(&self, profile: Profile) -> Result<(), String> {
        let key = self.ivars().key.borrow().clone();
        let owner = self
            .ivars()
            .owner
            .load()
            .ok_or(tr!("The configuration editor has closed."))?;
        owner.save_profile(key.as_deref(), profile.clone())?;
        if let Some(window) = owner.ivars().window.get() {
            window.profiles.select(&profile.name);
        }
        *self.ivars().key.borrow_mut() = Some(profile.name.clone());
        *self.ivars().profile.borrow_mut() = profile;
        self.update();
        Ok(())
    }

    pub fn save_action(&self, index: Option<usize>, action: Action) -> Result<(), String> {
        let mut profile = self.ivars().profile.borrow().clone();
        match index {
            Some(index) => profile.actions[index] = action,
            None => profile.actions.push(action),
        }
        self.persist(profile)?;
        self.reload(index.or_else(|| self.ivars().profile.borrow().actions.len().checked_sub(1)));
        Ok(())
    }

    fn action_row(&self) -> isize {
        let clicked = self.ivars().actions.clickedRow();
        if clicked >= 0 {
            clicked
        } else {
            self.ivars().actions.selectedRow()
        }
    }

    fn open_action(&self, index: Option<usize>, action: Action) {
        self.finish_editing();
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
