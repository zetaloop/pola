use objc2::{AnyThread, MainThreadOnly, rc::Retained, runtime::ProtocolObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSEdgeInsets, NSIndexSet, NSSize, NSString};

use super::{Delegate, schedule, ui};
use crate::{config::Config, mode::Mode, schedule::Event};

pub struct Window {
    pub window: Retained<NSWindow>,
    split: Retained<NSSplitViewController>,
    content: Retained<NSViewController>,
    appearance: Retained<NSView>,
    pub schedule: Retained<schedule::Editor>,
    navigation: Retained<NSTableView>,
    inspector: Retained<NSSplitViewItem>,
    images: [Retained<NSImageView>; 2],
    proportions: Vec<Retained<NSLayoutConstraint>>,
    captions: [Retained<NSTextField>; 2],
    next: Retained<NSTextField>,
}

impl Window {
    pub fn new(mtm: MainThreadMarker, delegate: &Delegate) -> Self {
        let window = ui::window(mtm, "pola", 820.0, 500.0);
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::FullSizeContentView);
        window.setContentMinSize(NSSize::new(620.0, 420.0));
        window.setFrameAutosaveName(&NSString::from_str("main"));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        let toolbar =
            NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str("main"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(delegate)));
        window.setToolbar(Some(&toolbar));

        let navigation = NSTableView::new(mtm);
        navigation.setHeaderView(None);
        navigation.setStyle(NSTableViewStyle::SourceList);
        navigation.setUsesAutomaticRowHeights(true);
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
        sidebar.setMinimumThickness(150.0);
        sidebar.setMaximumThickness(210.0);
        sidebar.setCanCollapse(true);

        let images = [NSImageView::new(mtm), NSImageView::new(mtm)];
        let captions = [ui::label(mtm, "Light", 15.0), ui::label(mtm, "Dark", 15.0)];
        let cards = ui::stack(mtm, true, &[]);
        cards.setSpacing(20.0);
        cards.setDistribution(NSStackViewDistribution::FillEqually);
        let mut proportions = Vec::new();
        for (index, image) in images.iter().enumerate() {
            image.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            let background = NSBackgroundExtensionView::new(mtm);
            background.setContentView(Some(image));
            proportions.push(
                background
                    .heightAnchor()
                    .constraintEqualToAnchor_multiplier(&background.widthAnchor(), 0.85),
            );
            let edit = ui::button(mtm, "Edit…", delegate, sel!(editAppearance:));
            edit.setTag(index as isize);
            edit.setBordered(false);
            let bar = ui::stack(mtm, true, &[&captions[index], &edit]);
            bar.setDistribution(NSStackViewDistribution::EqualSpacing);
            bar.setEdgeInsets(NSEdgeInsets {
                top: 8.0,
                left: 14.0,
                bottom: 8.0,
                right: 14.0,
            });
            let glass = NSGlassEffectView::new(mtm);
            glass.setContentView(Some(&bar));
            glass.setCornerRadius(999.0);
            unsafe {
                let _: () = objc2::msg_send![&glass, setEffectIsInteractive: true];
            }
            background.addSubview(&glass);
            glass.setTranslatesAutoresizingMaskIntoConstraints(false);
            background
                .heightAnchor()
                .constraintGreaterThanOrEqualToAnchor_multiplier_constant(
                    &glass.heightAnchor(),
                    1.0,
                    16.0,
                )
                .setActive(true);
            NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
                glass
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&background.leadingAnchor(), 8.0),
                glass
                    .trailingAnchor()
                    .constraintEqualToAnchor_constant(&background.trailingAnchor(), -8.0),
                glass
                    .bottomAnchor()
                    .constraintEqualToAnchor_constant(&background.bottomAnchor(), -8.0),
            ]));
            cards.addArrangedSubview(&background);
        }
        let glass_group = NSGlassEffectContainerView::new(mtm);
        glass_group.setContentView(Some(&cards));
        let title = ui::label(mtm, "Appearance", 26.0);
        let next = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        next.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let schedule = ui::button(mtm, "Edit schedule…", delegate, sel!(showSchedule:));
        let summary = ui::stack(mtm, true, &[&next, &schedule]);
        summary.setDistribution(NSStackViewDistribution::EqualSpacing);
        let body = ui::stack(mtm, false, &[&title, &glass_group, &summary]);
        body.setSpacing(16.0);
        for view in [&*glass_group as &NSView, &*summary] {
            view.widthAnchor()
                .constraintEqualToAnchor(&body.widthAnchor())
                .setActive(true);
        }
        let appearance = NSView::new(mtm);
        ui::mount(&appearance, &body, 24.0);
        let content = NSViewController::new(mtm);
        content.setView(&appearance);
        let content_item = NSSplitViewItem::splitViewItemWithViewController(&content);
        content_item.setAutomaticallyAdjustsSafeAreaInsets(true);
        content_item.setMinimumThickness(460.0);

        let inspector_view = NSViewController::new(mtm);
        inspector_view.setView(&NSView::new(mtm));
        let inspector = NSSplitViewItem::inspectorWithViewController(&inspector_view);
        inspector.setMinimumThickness(300.0);
        inspector.setMaximumThickness(420.0);
        inspector.setCanCollapse(true);
        inspector.setCollapsed(true);
        let split = NSSplitViewController::new(mtm);
        split.addSplitViewItem(&sidebar);
        split.addSplitViewItem(&content_item);
        split.addSplitViewItem(&inspector);
        window.setContentViewController(Some(&split));
        navigation.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
        let schedule = schedule::Editor::new(mtm, delegate);
        Self {
            window,
            split,
            content,
            appearance,
            schedule,
            navigation,
            inspector,
            images,
            proportions,
            captions,
            next,
        }
    }

    pub fn show_page(&self, page: isize) {
        self.content.setView(if page == 1 {
            self.schedule.view()
        } else {
            &self.appearance
        });
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

    pub fn inspect(&self, view: &NSView) {
        self.show_page(0);
        self.inspector
            .viewController(self.window.mtm())
            .setView(view);
        if self.inspector.isCollapsed() {
            unsafe { self.split.toggleInspector(None) };
        }
    }

    pub fn close_inspector(&self) {
        if !self.inspector.isCollapsed() {
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
                    format!("Next · {} · {}", event.at.strftime("%a %H:%M"), event.mode)
                })
                .unwrap_or_else(|| "Add an arrangement to enable automatic switching.".into())
            } else {
                "Schedule is off".into()
            }));
        let workspace = NSWorkspace::sharedWorkspace();
        let desktop = NSScreen::mainScreen(self.window.mtm())
            .and_then(|screen| workspace.desktopImageURLForScreen(&screen))
            .and_then(|url| url.path());
        for (index, profile) in [&config.light, &config.dark].into_iter().enumerate() {
            let path = profile
                .wallpaper
                .as_ref()
                .map(|path| NSString::from_str(&path.to_string_lossy()))
                .or_else(|| desktop.clone());
            let image =
                path.and_then(|path| NSImage::initWithContentsOfFile(NSImage::alloc(), &path));
            self.proportions[index].setActive(image.is_some());
            self.images[index].setImage(image.as_deref());
            let title = if index == 0 { "Light" } else { "Dark" };
            let selected = (index == 0) == (mode == Mode::Light);
            self.captions[index].setStringValue(&NSString::from_str(&if selected {
                format!("{title} · Active")
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
