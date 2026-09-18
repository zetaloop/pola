use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::{ProtocolObject, Sel},
};
use objc2_app_kit::{
    NSApplication, NSControlTextEditingDelegate, NSDragOperation, NSDraggingDestination,
    NSDraggingInfo, NSImage, NSImageScaling, NSImageView, NSLayoutAttribute,
    NSPasteboardTypeFileURL, NSStackView, NSTextField, NSTextFieldDelegate,
    NSUserInterfaceLayoutOrientation,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSString, NSURL,
};

use super::ui;

pub struct Ivars {
    field: Retained<NSTextField>,
    image: Option<Retained<NSImageView>>,
    target: Weak<NSObject>,
    action: Sel,
}

define_class!(
    #[unsafe(super = NSStackView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct FileInput;

    unsafe impl NSObjectProtocol for FileInput {}
    unsafe impl NSTextFieldDelegate for FileInput {}
    unsafe impl NSControlTextEditingDelegate for FileInput {
        #[unsafe(method(controlTextDidEndEditing:))]
        fn end_editing(&self, _notification: &NSNotification) {
            self.changed();
        }
    }

    unsafe impl NSDraggingDestination for FileInput {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if file_path(sender).is_some() {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_drag(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            file_path(sender).is_some()
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            if let Some(path) = file_path(sender) {
                self.ivars().field.setStringValue(&path);
                self.changed();
                true
            } else {
                false
            }
        }
    }
);

impl FileInput {
    pub fn new(
        mtm: MainThreadMarker,
        preview: bool,
        target: &NSObject,
        action: Sel,
    ) -> Retained<Self> {
        let field = NSTextField::textFieldWithString(&NSString::new(), mtm);
        field.setPlaceholderString(Some(&NSString::from_str("Drop a file or enter its path")));
        let image = preview
            .then(|| NSImageView::imageViewWithImage(&ui::symbol("photo", "Wallpaper"), mtm));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            field,
            image,
            target: Weak::new(target),
            action,
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        this.setAlignment(NSLayoutAttribute::Leading);
        this.setSpacing(8.0);
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        if let Some(image) = &this.ivars().image {
            image.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            image
                .heightAnchor()
                .constraintEqualToConstant(180.0)
                .setActive(true);
            image
                .widthAnchor()
                .constraintEqualToAnchor(&this.widthAnchor())
                .setActive(true);
            this.addArrangedSubview(image);
        }
        let field = &this.ivars().field;
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this.addArrangedSubview(field);
        field
            .widthAnchor()
            .constraintEqualToAnchor(&this.widthAnchor())
            .setActive(true);
        this
    }

    pub fn value(&self) -> String {
        self.ivars().field.stringValue().to_string()
    }

    pub fn set_value(&self, path: &str) {
        self.ivars().field.setStringValue(&NSString::from_str(path));
        self.update_image();
    }

    pub fn update_image(&self) {
        if let Some(view) = &self.ivars().image {
            let image = NSImage::initWithContentsOfFile(
                NSImage::alloc(),
                &self.ivars().field.stringValue(),
            )
            .unwrap_or_else(|| ui::symbol("photo", "Wallpaper"));
            view.setImage(Some(&image));
        }
    }

    fn changed(&self) {
        if let Some(target) = self.ivars().target.load() {
            unsafe {
                NSApplication::sharedApplication(self.mtm()).sendAction_to_from(
                    self.ivars().action,
                    Some(&target),
                    Some(self),
                );
            }
        }
    }
}

fn file_path(sender: &ProtocolObject<dyn NSDraggingInfo>) -> Option<Retained<NSString>> {
    let items = sender.draggingPasteboard().pasteboardItems()?;
    if items.len() != 1 {
        return None;
    }
    let value = items
        .objectAtIndex(0)
        .stringForType(unsafe { NSPasteboardTypeFileURL })?;
    let url = NSURL::URLWithString(&value)?;
    if url.isFileURL() { url.path() } else { None }
}
