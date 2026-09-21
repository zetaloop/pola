use std::cell::RefCell;

use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};

use super::{Delegate, profile, ui};
use crate::config::Profile;

pub struct Ivars {
    owner: Weak<Delegate>,
    profiles: RefCell<Vec<Profile>>,
    table: Retained<NSTableView>,
    pages: Retained<NSTabViewController>,
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
        let table = ui::table(mtm, "Configurations");
        let pages = NSTabViewController::new(mtm);
        pages.setTabStyle(NSTabViewControllerTabStyle::Unspecified);
        pages
            .tabView()
            .setTabViewType(NSTabViewType::NoTabsNoBorder);
        pages.tabView().setDrawsBackground(false);
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            profiles: RefCell::new(Vec::new()),
            table,
            pages,
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
        let title = ui::heading(mtm, "Configurations");
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&this.ivars().table));
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(180.0)
            .setActive(true);
        let add = ui::button(mtm, "New configuration", &this, sel!(addProfile:));
        let content = ui::stack(mtm, false, &[&title, &scroll, &add]);
        scroll
            .widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        let view = NSView::new(mtm);
        ui::mount(&view, &content, 24.0);
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
        if let Some(owner) = self.ivars().owner.load() {
            let profiles = owner.config().profiles;
            if *self.ivars().profiles.borrow() != profiles {
                *self.ivars().profiles.borrow_mut() = profiles;
                self.ivars().table.reloadData();
            }
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
        let controller = NSViewController::new(self.mtm());
        controller.setView(editor.view());
        self.ivars()
            .pages
            .addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(&controller));
        self.ivars().pages.setSelectedTabViewItemIndex(1);
        editor.focus();
        *self.ivars().editor.borrow_mut() = Some(editor);
    }
}
