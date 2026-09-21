use crate::locale::tr;

use block2::RcBlock;
use objc2::{
    MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::{ProtocolObject, Sel},
};
use objc2_app_kit::{
    NSControlTextEditingDelegate, NSDragOperation, NSDraggingDestination, NSDraggingInfo,
    NSModalResponseAbort, NSModalResponseOK, NSOpenPanel, NSPasteboardTypeFileURL, NSTextField,
    NSTextFieldBezelStyle, NSTextFieldDelegate,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSString, NSURL,
};

define_class!(
    #[unsafe(super = NSTextField)]
    #[thread_kind = MainThreadOnly]
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
        fn entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if file_path(sender).is_some() {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }
        #[unsafe(method(prepareForDragOperation:))]
        fn prepare(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            file_path(sender).is_some()
        }
        #[unsafe(method(performDragOperation:))]
        fn perform(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            if let Some(path) = file_path(sender) {
                self.setStringValue(&path);
                self.changed();
                true
            } else {
                false
            }
        }
    }
);

impl FileInput {
    pub fn new(mtm: MainThreadMarker, target: &NSObject, action: Sel) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBezeled(true);
        this.setBezelStyle(NSTextFieldBezelStyle::RoundedBezel);
        this.setEditable(true);
        this.setSelectable(true);
        this.setPlaceholderString(Some(&NSString::from_str(tr!(
            "Drop a file or enter its path"
        ))));
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        unsafe {
            this.setDelegate(Some(ProtocolObject::from_ref(&*this)));
            this.setTarget(Some(target));
            this.setAction(Some(action));
        }
        this
    }
    pub fn choose(&self, failed: impl Fn(String) + 'static) {
        let Some(window) = self.window() else { return };
        let panel = NSOpenPanel::openPanel(self.mtm());
        let selected = panel.clone();
        let field = Weak::new(self);
        let completed = RcBlock::new(move |response| {
            if response == NSModalResponseAbort {
                failed(tr!("Could not open the file picker.").into());
            } else if response == NSModalResponseOK
                && let Some(field) = field.load()
                && let Some(path) = selected.URL().and_then(|url| url.path())
            {
                field.setStringValue(&path);
                field.changed();
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &completed);
    }

    pub fn value(&self) -> String {
        self.stringValue().to_string()
    }
    pub fn set_value(&self, value: &str) {
        self.setStringValue(&NSString::from_str(value));
    }
    fn changed(&self) {
        if let Some(action) = self.action() {
            unsafe {
                self.sendAction_to(Some(action), self.target().as_deref());
            }
        }
    }
}

pub fn file_path(sender: &ProtocolObject<dyn NSDraggingInfo>) -> Option<Retained<NSString>> {
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
