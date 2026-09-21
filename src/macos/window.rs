use objc2::{MainThreadOnly, rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSIndexSet, NSSize, ns_string};

use super::{Delegate, appearance::Appearance, profiles, schedule, settings::Settings, ui};
use crate::{mode::Mode, schedule::Event};

pub struct Window {
    pub window: Retained<NSWindow>,
    content: Retained<NSTabViewController>,
    pub schedule: Retained<schedule::Editor>,
    pub profiles: Retained<profiles::List>,
    pub settings: Retained<Settings>,
    navigation: Retained<NSTableView>,
    appearance: Appearance,
}

impl Window {
    pub fn new(mtm: MainThreadMarker, delegate: &Delegate) -> Self {
        let window = ui::window(mtm, "pola", 680.0, 480.0);
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::FullSizeContentView);
        window.setContentMinSize(NSSize::new(420.0, 360.0));
        window.setFrameAutosaveName(ns_string!("main"));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        window.setDelegate(Some(ProtocolObject::from_ref(delegate)));

        let navigation = NSTableView::new(mtm);
        navigation.setHeaderView(None);
        navigation.setStyle(NSTableViewStyle::SourceList);
        navigation.setBackgroundColor(NSColor::clearColor().as_ref());
        navigation.setRowSizeStyle(NSTableViewRowSizeStyle::Default);
        navigation.setAllowsEmptySelection(false);
        navigation.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let column =
            NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("navigation"));
        navigation.addTableColumn(&column);
        unsafe {
            navigation.setDataSource(Some(ProtocolObject::from_ref(delegate)));
            navigation.setDelegate(Some(ProtocolObject::from_ref(delegate)));
        }
        let sidebar_scroll = NSScrollView::new(mtm);
        sidebar_scroll.setDrawsBackground(false);
        sidebar_scroll.setDocumentView(Some(&navigation));
        let sidebar = NSViewController::new(mtm);
        sidebar.setView(&sidebar_scroll);
        let sidebar = NSSplitViewItem::sidebarWithViewController(&sidebar);

        let appearance = Appearance::new(mtm, delegate);
        let schedule = schedule::Editor::new(mtm, delegate);
        let profiles = profiles::List::new(mtm, delegate);
        let settings = Settings::new(mtm, delegate);
        let content = ui::pages(mtm, &[&appearance.view, settings.view()]);
        content.insertTabViewItem_atIndex(
            &NSTabViewItem::tabViewItemWithViewController(profiles.controller()),
            1,
        );
        content.insertTabViewItem_atIndex(
            &NSTabViewItem::tabViewItemWithViewController(schedule.controller()),
            2,
        );
        let content_item = NSSplitViewItem::splitViewItemWithViewController(&content);
        content_item.setMinimumThickness(350.0);
        content_item.setAutomaticallyAdjustsSafeAreaInsets(true);
        let split = NSSplitViewController::new(mtm);
        split.addSplitViewItem(&sidebar);
        split.addSplitViewItem(&content_item);
        window.setContentViewController(Some(&split));

        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), ns_string!("main"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(delegate)));
        window.setToolbar(Some(&toolbar));
        toolbar.setVisible(true);
        navigation.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
        Self {
            window,
            content,
            schedule,
            profiles,
            settings,
            navigation,
            appearance,
        }
    }

    pub fn show_page(&self, page: isize) {
        if page != 3 {
            self.settings.finish();
        }
        self.window.makeFirstResponder(None);
        self.content.setSelectedTabViewItemIndex(page);
        if self.navigation.selectedRow() != page {
            self.navigation.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(page as usize),
                false,
            );
        }
    }

    pub fn update(&self, mode: Mode, next: Option<&Event>) {
        self.appearance.update(mode, next);
        self.profiles.update();
        self.schedule.update();
        self.settings.update();
    }

    pub fn show(&self) {
        ui::show(&self.window);
    }
}
