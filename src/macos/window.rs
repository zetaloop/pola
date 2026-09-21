use objc2::{MainThreadOnly, rc::Retained, runtime::ProtocolObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSIndexSet, NSSize, NSString, ns_string};

use super::{Delegate, profiles, schedule, ui};
use crate::{mode::Mode, schedule::Event};

pub struct Window {
    pub window: Retained<NSWindow>,
    content: Retained<NSTabViewController>,
    pub schedule: Retained<schedule::Editor>,
    pub profiles: Retained<profiles::List>,
    navigation: Retained<NSTableView>,
    mode: Retained<NSSegmentedControl>,
    next: Retained<NSTextField>,
}

impl Window {
    pub fn new(mtm: MainThreadMarker, delegate: &Delegate) -> Self {
        let window = ui::window(mtm, "pola", 820.0, 500.0);
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::FullSizeContentView);
        window.setContentMinSize(NSSize::new(620.0, 420.0));
        window.setFrameAutosaveName(ns_string!("main"));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);

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

        let title = ui::heading(mtm, "Appearance");
        let mode = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_slice(&[ns_string!("Light"), ns_string!("Dark")]),
                NSSegmentSwitchTracking::SelectOne,
                Some(delegate),
                Some(sel!(selectMode:)),
                mtm,
            )
        };
        mode.setControlSize(NSControlSize::Large);
        let next = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        next.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let body = ui::stack(mtm, false, &[&title, &mode, &next]);
        body.setSpacing(24.0);
        next.widthAnchor()
            .constraintEqualToAnchor(&body.widthAnchor())
            .setActive(true);
        let appearance = NSView::new(mtm);
        ui::mount(&appearance, &body, 24.0);

        let schedule = schedule::Editor::new(mtm, delegate);
        let profiles = profiles::List::new(mtm, delegate);
        let content = ui::pages(mtm, &[&appearance, schedule.view()]);
        content.insertTabViewItem_atIndex(
            &NSTabViewItem::tabViewItemWithViewController(profiles.controller()),
            1,
        );
        let content_item = NSSplitViewItem::splitViewItemWithViewController(&content);
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
            navigation,
            mode,
            next,
        }
    }

    pub fn show_page(&self, page: isize) {
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
        self.mode
            .setSelectedSegment(isize::from(mode == Mode::Dark));
        self.next.setStringValue(&NSString::from_str(
            &next
                .map(|event| {
                    format!(
                        "Switch to {} at {}",
                        event.mode,
                        event.at.strftime("%a %H:%M")
                    )
                })
                .unwrap_or_default(),
        ));
        self.profiles.update();
        self.schedule.update();
    }

    pub fn show(&self) {
        ui::show(&self.window);
    }
}
