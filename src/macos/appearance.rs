use objc2::{rc::Retained, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSString};

use super::{Delegate, ui};
use crate::{
    locale::{self, tr},
    mode::Mode,
    schedule::Event,
};

pub struct Appearance {
    pub view: Retained<NSView>,
    mode: Retained<NSSegmentedControl>,
    next: Retained<NSTextField>,
}

impl Appearance {
    pub fn new(mtm: MainThreadMarker, delegate: &Delegate) -> Self {
        let view = NSView::new(mtm);
        let title = ui::heading(mtm, tr!("Appearance"));
        let mode = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&[
                    NSString::from_str(tr!("Light")),
                    NSString::from_str(tr!("Dark")),
                ]),
                NSSegmentSwitchTracking::SelectOne,
                Some(delegate),
                Some(sel!(selectMode:)),
                mtm,
            )
        };
        mode.setControlSize(NSControlSize::ExtraLarge);
        let next = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        next.setTextColor(Some(&NSColor::secondaryLabelColor()));
        next.setAlignment(NSTextAlignment::Center);
        let body = ui::stack(mtm, false, &[&title, &mode, &next]);
        body.setAlignment(NSLayoutAttribute::CenterX);
        view.addSubview(&body);
        body.setTranslatesAutoresizingMaskIntoConstraints(false);
        let margins = view.layoutMarginsGuide();
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            body.leadingAnchor()
                .constraintEqualToAnchor(&margins.leadingAnchor()),
            body.trailingAnchor()
                .constraintEqualToAnchor(&margins.trailingAnchor()),
            body.centerYAnchor()
                .constraintEqualToAnchor(&margins.centerYAnchor()),
            body.topAnchor()
                .constraintGreaterThanOrEqualToAnchor(&margins.topAnchor()),
            body.bottomAnchor()
                .constraintLessThanOrEqualToAnchor(&margins.bottomAnchor()),
            next.widthAnchor()
                .constraintEqualToAnchor(&body.widthAnchor()),
        ]));
        Self { view, mode, next }
    }

    pub fn update(&self, mode: Mode, next: Option<&Event>) {
        self.mode
            .setSelectedSegment(isize::from(mode == Mode::Dark));
        self.next.setHidden(next.is_none());
        self.next.setStringValue(&NSString::from_str(
            &next
                .map(|event| locale::next(event).unwrap_or_else(|error| error))
                .unwrap_or_default(),
        ));
    }
}
