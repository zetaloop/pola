use objc2::{
    MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Sel},
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDictionary, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct Content;
    unsafe impl NSObjectProtocol for Content {}
    impl Content {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
    }
);

pub fn scroll(parent: &NSView, content: &NSView) {
    let mtm = parent.mtm();
    let document: Retained<Content> =
        unsafe { msg_send![super(Content::alloc(mtm).set_ivars(())), init] };
    document.setTranslatesAutoresizingMaskIntoConstraints(false);
    let scroll = NSScrollView::new(mtm);
    scroll.setDrawsBackground(false);
    scroll.setHasVerticalScroller(true);
    scroll.setDocumentView(Some(&document));
    document
        .widthAnchor()
        .constraintEqualToAnchor(&scroll.contentView().widthAnchor())
        .setActive(true);
    mount(&document, content);
    content
        .bottomAnchor()
        .constraintEqualToAnchor(&document.layoutMarginsGuide().bottomAnchor())
        .setActive(true);
    parent.addSubview(&scroll);
    scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        scroll
            .leadingAnchor()
            .constraintEqualToAnchor(&parent.leadingAnchor()),
        scroll
            .trailingAnchor()
            .constraintEqualToAnchor(&parent.trailingAnchor()),
        scroll
            .topAnchor()
            .constraintEqualToAnchor(&parent.topAnchor()),
        scroll
            .bottomAnchor()
            .constraintEqualToAnchor(&parent.bottomAnchor()),
    ]));
}

pub fn window(mtm: MainThreadMarker, title: &str, width: f64, height: f64) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::ZERO, NSSize::new(width, height)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str(title));
    window.setAutorecalculatesKeyViewLoop(true);
    unsafe { window.setReleasedWhenClosed(false) };
    window.center();
    window
}

pub fn show(window: &NSWindow) {
    window.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(window.mtm()).activate();
}

pub fn stack(mtm: MainThreadMarker, horizontal: bool, views: &[&NSView]) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(if horizontal {
        NSUserInterfaceLayoutOrientation::Horizontal
    } else {
        NSUserInterfaceLayoutOrientation::Vertical
    });
    stack.setAlignment(if horizontal {
        NSLayoutAttribute::CenterY
    } else {
        NSLayoutAttribute::Leading
    });
    stack.setSpacing(12.0);
    stack
}

pub fn form(mtm: MainThreadMarker, rows: &[[&NSView; 2]]) -> Retained<NSGridView> {
    let rows: Vec<_> = rows.iter().map(|row| NSArray::from_slice(row)).collect();
    let form = NSGridView::gridViewWithViews(&NSArray::from_retained_slice(&rows), mtm);
    form.setRowSpacing(16.0);
    form.columnAtIndex(0)
        .setXPlacement(NSGridCellPlacement::Leading);
    form.columnAtIndex(1)
        .setXPlacement(NSGridCellPlacement::Fill);
    form
}

pub fn actions(mtm: MainThreadMarker, views: &[&NSView]) -> Retained<NSStackView> {
    let actions = stack(mtm, true, &[]);
    actions.setViews_inGravity(&NSArray::from_slice(views), NSStackViewGravity::Trailing);
    actions
}

pub fn mount(parent: &NSView, child: &NSView) {
    parent.addSubview(child);
    child.setTranslatesAutoresizingMaskIntoConstraints(false);
    let region = parent.layoutGuideForLayoutRegion(
        &NSViewLayoutRegion::marginsLayoutRegionWithCornerAdaptation(
            NSViewLayoutRegionAdaptivityAxis::Horizontal,
        ),
    );
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        child
            .leadingAnchor()
            .constraintEqualToAnchor(&region.leadingAnchor()),
        child
            .trailingAnchor()
            .constraintEqualToAnchor(&region.trailingAnchor()),
        child
            .topAnchor()
            .constraintEqualToAnchor(&region.topAnchor()),
        child
            .bottomAnchor()
            .constraintLessThanOrEqualToAnchor(&region.bottomAnchor()),
    ]));
}

pub fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

pub fn heading(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    let label = label(mtm, text);
    let font = unsafe {
        NSFont::preferredFontForTextStyle_options(NSFontTextStyleTitle2, &NSDictionary::new())
    };
    label.setFont(Some(&font));
    label
}

pub fn table(mtm: MainThreadMarker, title: &str) -> Retained<NSTableView> {
    let table = NSTableView::new(mtm);
    table.setHeaderView(None);
    table.setStyle(NSTableViewStyle::Inset);
    table.setRowSizeStyle(NSTableViewRowSizeStyle::Default);
    table.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    let column =
        NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &NSString::from_str(title));
    column.setTitle(&NSString::from_str(title));
    table.addTableColumn(&column);
    table
}

pub fn cell(
    mtm: MainThreadMarker,
    text: &str,
    image: Option<&NSImage>,
) -> Retained<NSTableCellView> {
    let cell = NSTableCellView::new(mtm);
    let text = label(mtm, text);
    cell.addSubview(&text);
    unsafe { cell.setTextField(Some(&text)) };
    if let Some(image) = image {
        let image = NSImageView::imageViewWithImage(image, mtm);
        cell.addSubview(&image);
        unsafe { cell.setImageView(Some(&image)) };
    }
    cell
}

pub fn pages(mtm: MainThreadMarker, views: &[&NSView]) -> Retained<NSTabViewController> {
    let pages = NSTabViewController::new(mtm);
    pages.setTabStyle(NSTabViewControllerTabStyle::Unspecified);
    pages
        .tabView()
        .setTabViewType(NSTabViewType::NoTabsNoBorder);
    pages.tabView().setDrawsBackground(false);
    for view in views {
        let controller = NSViewController::new(mtm);
        controller.setView(view);
        pages.addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(&controller));
    }
    pages
}

pub fn button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    }
}

pub fn symbol(name: &str, description: &str) -> Retained<NSImage> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        Some(&NSString::from_str(description)),
    )
    .expect("system symbol unavailable")
}
