use objc2::{AnyThread, MainThreadOnly, rc::Retained, runtime::ProtocolObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSIndexSet, NSSize, NSString};

use super::{Delegate, profile, schedule, ui};
use crate::{config::Config, mode::Mode, schedule::Event};

pub struct Window {
    pub window: Retained<NSWindow>,
    split: Retained<NSSplitViewController>,
    content: Retained<NSTabViewController>,
    pub schedule: Retained<schedule::Editor>,
    navigation: Retained<NSTableView>,
    inspector: Retained<NSSplitViewItem>,
    inspector_pages: Retained<NSTabViewController>,
    _profiles: [Retained<profile::Editor>; 2],
    previews: [Retained<NSButton>; 2],
    next: Retained<NSTextField>,
}

impl Window {
    pub fn new(mtm: MainThreadMarker, delegate: &Delegate) -> Self {
        let window = ui::window(mtm, "pola", 820.0, 500.0);
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::FullSizeContentView);
        window.setContentMinSize(NSSize::new(620.0, 420.0));
        window.setFrameAutosaveName(&NSString::from_str("main"));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);

        let navigation = NSTableView::new(mtm);
        navigation.setHeaderView(None);
        navigation.setStyle(NSTableViewStyle::SourceList);
        navigation.setBackgroundColor(NSColor::clearColor().as_ref());
        navigation.setRowSizeStyle(NSTableViewRowSizeStyle::Default);
        navigation.setAllowsEmptySelection(false);
        navigation.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("navigation"),
        );
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

        let previews = [
            ui::button(mtm, "Light", delegate, sel!(editAppearance:)),
            ui::button(mtm, "Dark", delegate, sel!(editAppearance:)),
        ];
        for (index, preview) in previews.iter().enumerate() {
            preview.setTag(index as isize);
            preview.setControlSize(NSControlSize::Large);
            preview.setBezelStyle(NSBezelStyle::FlexiblePush);
            preview.setBorderShape(NSControlBorderShape::RoundedRectangle);
            preview.setImagePosition(NSCellImagePosition::ImageAbove);
            preview.setImageScaling(NSImageScaling::ScaleProportionallyDown);
            preview.setToolTip(Some(&NSString::from_str(if index == 0 {
                "Edit light appearance"
            } else {
                "Edit dark appearance"
            })));
            let appearance = NSAppearance::appearanceNamed(unsafe {
                if index == 0 {
                    NSAppearanceNameAqua
                } else {
                    NSAppearanceNameDarkAqua
                }
            });
            preview.setAppearance(appearance.as_deref());
        }
        let choices = ui::stack(mtm, true, &[&previews[0], &previews[1]]);
        choices.setDistribution(NSStackViewDistribution::FillEqually);
        let title = ui::heading(mtm, "Appearance");
        let next = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        next.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let schedule_action = ui::button(mtm, "Edit schedule…", delegate, sel!(showSchedule:));
        let summary = ui::stack(mtm, true, &[&next, &schedule_action]);
        summary.setDistribution(NSStackViewDistribution::EqualSpacing);
        let body = ui::stack(mtm, false, &[&title, &choices, &summary]);
        body.setSpacing(24.0);
        for row in [&*choices, &*summary] {
            row.widthAnchor()
                .constraintEqualToAnchor(&body.widthAnchor())
                .setActive(true);
        }
        let appearance = NSView::new(mtm);
        ui::mount(&appearance, &body, 24.0);
        let schedule = schedule::Editor::new(mtm, delegate);
        let content = ui::pages(mtm, &[&appearance, schedule.view()]);
        let content_item = NSSplitViewItem::splitViewItemWithViewController(&content);
        content_item.setAutomaticallyAdjustsSafeAreaInsets(true);

        let config = delegate.config();
        let profiles = [
            profile::Editor::new(mtm, delegate, Mode::Light, config.light),
            profile::Editor::new(mtm, delegate, Mode::Dark, config.dark),
        ];
        let inspector_pages = ui::pages(mtm, &[profiles[0].view(), profiles[1].view()]);
        inspector_pages
            .setSelectedTabViewItemIndex(isize::from(delegate.system_mode() == Mode::Dark));
        let inspector = NSSplitViewItem::inspectorWithViewController(&inspector_pages);
        inspector.setCollapsed(true);
        let split = NSSplitViewController::new(mtm);
        split.addSplitViewItem(&sidebar);
        split.addSplitViewItem(&content_item);
        split.addSplitViewItem(&inspector);
        window.setContentViewController(Some(&split));

        let toolbar =
            NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str("main"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(delegate)));
        window.setToolbar(Some(&toolbar));
        toolbar.setVisible(true);
        navigation.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
        Self {
            window,
            split,
            content,
            schedule,
            navigation,
            inspector,
            inspector_pages,
            _profiles: profiles,
            previews,
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
        if page == 1 {
            self.inspector.setCollapsed(true);
        }
    }

    pub fn inspect(&self, mode: Mode) {
        self.show_page(0);
        self.inspector_pages
            .setSelectedTabViewItemIndex(isize::from(mode == Mode::Dark));
        if self.inspector.isCollapsed() {
            unsafe { self.split.toggleInspector(None) };
        }
    }

    pub fn update(&self, config: &Config, mode: Mode, next: Option<&Event>) {
        if let Some(toolbar) = self.window.toolbar() {
            for item in toolbar.items() {
                if item.itemIdentifier().to_string() == "mode"
                    && let Some(view) = item.view()
                    && let Ok(control) = view.downcast::<NSSegmentedControl>()
                {
                    control.setSelectedSegment(isize::from(mode == Mode::Dark));
                }
            }
        }
        self.next
            .setStringValue(&NSString::from_str(&if config.schedule.enabled {
                next.map(|event| {
                    format!(
                        "Switch to {} at {}",
                        event.mode,
                        event.at.strftime("%a %H:%M")
                    )
                })
                .unwrap_or_else(|| "Add an arrangement to enable automatic switching.".into())
            } else {
                "Schedule is off".into()
            }));
        for (index, profile) in [&config.light, &config.dark].into_iter().enumerate() {
            let image = profile.wallpaper.as_ref().and_then(|path| {
                NSImage::initWithContentsOfFile(
                    NSImage::alloc(),
                    &NSString::from_str(&path.to_string_lossy()),
                )
            });
            if let Some(image) = &image {
                let size = image.size();
                let scale = (160.0 / size.width).min(100.0 / size.height).min(1.0);
                image.setSize(NSSize::new(size.width * scale, size.height * scale));
            }
            self.previews[index].setImage(image.as_deref());
            let title = if index == 0 { "Light" } else { "Dark" };
            let selected = (index == 0) == (mode == Mode::Light);
            self.previews[index].setTitle(&NSString::from_str(&if selected {
                format!("{title} (active)")
            } else {
                title.into()
            }));
        }
        self.schedule.update();
    }

    pub fn show(&self) {
        ui::show(&self.window);
    }
}
