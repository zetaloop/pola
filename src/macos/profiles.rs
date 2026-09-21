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
    MainThreadMarker, NSArray, NSIndexSet, NSObject, NSObjectProtocol, NSString, ns_string,
};

use super::{Delegate, profile, ui};
use crate::config::Profile;

pub struct Ivars {
    owner: Weak<Delegate>,
    profiles: RefCell<Vec<Profile>>,
    table: Retained<NSTableView>,
    pages: Retained<NSTabViewController>,
    error: Retained<NSTextField>,
    editor: RefCell<Option<Retained<profile::Editor>>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct List;

    unsafe impl NSObjectProtocol for List {}
    unsafe impl NSControlTextEditingDelegate for List {}
    unsafe impl NSTableViewDataSource for List {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _table: &NSTableView) -> isize {
            self.ivars().profiles.borrow().len() as isize
        }

        #[unsafe(method_id(tableView:pasteboardWriterForRow:))]
        fn pasteboard(&self, _table: &NSTableView, row: isize) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
            let item = NSPasteboardItem::new();
            item.setString_forType(&NSString::from_str(&row.to_string()), ns_string!("io.github.zetaloop.pola.profile"));
            Some(ProtocolObject::from_retained(item))
        }

        #[unsafe(method(tableView:validateDrop:proposedRow:proposedDropOperation:))]
        fn validate_drop(&self, table: &NSTableView, info: &ProtocolObject<dyn NSDraggingInfo>, row: isize, _operation: NSTableViewDropOperation) -> NSDragOperation {
            let local = info.draggingSource().and_then(|source| source.downcast::<NSTableView>().ok())
                .is_some_and(|source| std::ptr::eq(&*source, table));
            if local && row >= 0 && row as usize <= self.ivars().profiles.borrow().len() {
                table.setDropRow_dropOperation(row, NSTableViewDropOperation::Above);
                NSDragOperation::Move
            } else { NSDragOperation::None }
        }

        #[unsafe(method(tableView:acceptDrop:row:dropOperation:))]
        fn accept_drop(&self, _table: &NSTableView, info: &ProtocolObject<dyn NSDraggingInfo>, row: isize, _operation: NSTableViewDropOperation) -> bool {
            if let Some(owner) = self.ivars().owner.load()
                && let Some(index) = info.draggingPasteboard().stringForType(ns_string!("io.github.zetaloop.pola.profile"))
                    .and_then(|value| value.to_string().parse::<usize>().ok())
            {
                let mut config = owner.config();
                if row >= 0 && row as usize <= config.profiles.len() && index < config.profiles.len() {
                    let target = row as usize - usize::from(index < row as usize);
                    let profile = config.profiles.remove(index);
                    let name = profile.name.clone();
                    config.profiles.insert(target, profile);
                    match owner.save_config(config) {
                        Ok(()) => { self.select(&name); self.ivars().error.setStringValue(&NSString::new()); true }
                        Err(error) => { self.ivars().error.setStringValue(&NSString::from_str(&error)); false }
                    }
                } else { false }
            } else { false }
        }
    }
    unsafe impl NSTableViewDelegate for List {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
            self.ivars().profiles.borrow().get(row as usize)
                .map(|profile| ui::cell(self.mtm(), &profile.name, None).into_super())
        }
    }
    impl List {
        #[unsafe(method(editProfile:))]
        fn edit_profile(&self, _sender: &NSObject) {
            let row = self.ivars().table.clickedRow();
            if row >= 0 {
                let profile = self.ivars().profiles.borrow().get(row as usize).cloned();
                if let Some(profile) = profile { self.edit(Some(profile)); }
            }
        }
        #[unsafe(method(addProfile:))]
        fn add_profile(&self, _sender: &NSObject) {
            self.edit(None);
        }
    }
);

impl List {
    pub fn new(mtm: MainThreadMarker, owner: &Delegate) -> Retained<Self> {
        let table = ui::table(mtm, tr!("Configurations"));
        let pages = NSTabViewController::new(mtm);
        pages.setTabStyle(NSTabViewControllerTabStyle::Unspecified);
        pages
            .tabView()
            .setTabViewType(NSTabViewType::NoTabsNoBorder);
        pages.tabView().setDrawsBackground(false);
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            profiles: RefCell::new(Vec::new()),
            table,
            pages,
            error,
            editor: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe {
            this.ivars()
                .table
                .setDataSource(Some(ProtocolObject::from_ref(&*this)));
            this.ivars()
                .table
                .setDelegate(Some(ProtocolObject::from_ref(&*this)));
            this.ivars().table.setTarget(Some(&this));
            this.ivars().table.setAction(Some(sel!(editProfile:)));
        }
        this.ivars()
            .table
            .registerForDraggedTypes(&NSArray::from_slice(&[ns_string!(
                "io.github.zetaloop.pola.profile"
            )]));
        this.ivars()
            .table
            .setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
        let title = ui::heading(mtm, tr!("Configurations"));
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&this.ivars().table));
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(180.0)
            .setActive(true);
        let add = ui::button(mtm, tr!("New configuration"), &this, sel!(addProfile:));
        let header = ui::stack(mtm, true, &[&title, &add]);
        header.setDistribution(NSStackViewDistribution::EqualSpacing);
        let content = ui::stack(mtm, false, &[&header, &scroll, &this.ivars().error]);
        header
            .widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        this.ivars()
            .error
            .widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        scroll
            .widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        let view = NSView::new(mtm);
        ui::mount(&view, &content);
        let controller = NSViewController::new(mtm);
        controller.setView(&view);
        this.ivars()
            .pages
            .addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(&controller));
        this.update();
        this
    }

    pub fn controller(&self) -> &NSTabViewController {
        &self.ivars().pages
    }

    fn view(&self) -> Retained<NSView> {
        self.ivars().pages.view()
    }

    pub fn update(&self) {
        if let Some(editor) = self.ivars().editor.borrow().as_ref() {
            editor.update();
        }
        if let Some(owner) = self.ivars().owner.load() {
            let profiles = owner.config().profiles;
            if *self.ivars().profiles.borrow() != profiles {
                *self.ivars().profiles.borrow_mut() = profiles;
                self.ivars().table.reloadData();
            }
        }
    }

    pub fn select(&self, name: &str) {
        let index = self
            .ivars()
            .profiles
            .borrow()
            .iter()
            .position(|profile| profile.name == name);
        if let Some(index) = index {
            self.ivars().table.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(index),
                false,
            );
            self.ivars().table.scrollRowToVisible(index as isize);
        }
    }

    pub fn show_list(&self) {
        if let Some(window) = self.view().window() {
            window.makeFirstResponder(None);
        }
        self.ivars().pages.setSelectedTabViewItemIndex(0);
        let items = self.ivars().pages.tabViewItems();
        if items.len() > 1 {
            self.ivars()
                .pages
                .removeTabViewItem(&items.objectAtIndex(1));
        }
    }

    fn edit(&self, profile: Option<Profile>) {
        let Some(owner) = self.ivars().owner.load() else {
            return;
        };
        self.show_list();
        let name = profile.as_ref().map(|profile| profile.name.clone());
        let editor = profile::Editor::new(self.mtm(), &owner, name, profile.unwrap_or_default());
        self.ivars()
            .pages
            .addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(
                editor.controller(),
            ));
        self.ivars().pages.setSelectedTabViewItemIndex(1);
        editor.focus();
        *self.ivars().editor.borrow_mut() = Some(editor);
    }
}
