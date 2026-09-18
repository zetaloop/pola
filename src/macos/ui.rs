use objc2::{
    MainThreadOnly,
    rc::Retained,
    runtime::{AnyObject, Sel},
};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSFont, NSImage, NSLayoutAttribute,
    NSLayoutConstraint, NSScrollView, NSStackView, NSTextField, NSUserInterfaceLayoutOrientation,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSPoint, NSRect, NSSize, NSString};

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

pub fn mount(parent: &NSView, child: &NSView, margin: f64) {
    parent.addSubview(child);
    child.setTranslatesAutoresizingMaskIntoConstraints(false);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        child
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&parent.leadingAnchor(), margin),
        child
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&parent.trailingAnchor(), -margin),
        child
            .topAnchor()
            .constraintEqualToAnchor_constant(&parent.topAnchor(), margin),
        child
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&parent.bottomAnchor(), -margin),
    ]));
}

pub fn scroll(mtm: MainThreadMarker, content: &NSView, height: f64) -> Retained<NSScrollView> {
    let scroll = NSScrollView::new(mtm);
    scroll.setHasVerticalScroller(true);
    scroll.setDrawsBackground(false);
    scroll
        .heightAnchor()
        .constraintEqualToConstant(height)
        .setActive(true);
    scroll.setDocumentView(Some(content));
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    let clip = scroll.contentView();
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        content
            .leadingAnchor()
            .constraintEqualToAnchor(&clip.leadingAnchor()),
        content
            .topAnchor()
            .constraintEqualToAnchor(&clip.topAnchor()),
        content
            .widthAnchor()
            .constraintEqualToAnchor(&clip.widthAnchor()),
    ]));
    scroll
}

pub fn label(mtm: MainThreadMarker, text: &str, size: f64) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(size)));
    label
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
