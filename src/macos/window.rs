use std::{cell::RefCell, path::PathBuf};

use objc2::{AnyThread, MainThreadOnly, rc::Retained, runtime::ProtocolObject, sel};
use objc2_app_kit::{
    NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSImageScaling, NSImageView,
    NSSegmentSwitchTracking, NSSegmentedControl, NSStackViewDistribution, NSSwitch, NSTextField,
    NSToolbar, NSWindow, NSWindowToolbarStyle,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSSize, NSString};

use crate::{config::Config, mode::Mode, schedule::Event};

use super::{Delegate, ui};

pub struct Window {
    pub window: Retained<NSWindow>,
    mode: Retained<NSSegmentedControl>,
    next: Retained<NSTextField>,
    schedule: Retained<NSSwitch>,
    images: [Retained<NSImageView>; 2],
    paths: RefCell<[Option<PathBuf>; 2]>,
}

impl Window {
    pub fn new(mtm: MainThreadMarker, delegate: &Delegate) -> Self {
        let window = ui::window(mtm, "pola", 640.0, 420.0);
        window.setContentMinSize(NSSize::new(540.0, 380.0));
        window.setFrameAutosaveName(&NSString::from_str("main"));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        let toolbar =
            NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str("main"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(delegate)));
        window.setToolbar(Some(&toolbar));

        let title = ui::label(mtm, "Appearance", 24.0);
        let mode = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&[
                    NSString::from_str("Light"),
                    NSString::from_str("Dark"),
                ]),
                NSSegmentSwitchTracking::SelectOne,
                Some(delegate),
                Some(sel!(selectMode:)),
                mtm,
            )
        };
        let heading = ui::stack(mtm, true, &[&title, &mode]);
        heading.setDistribution(NSStackViewDistribution::EqualSpacing);

        let images = [
            NSImageView::imageViewWithImage(&ui::symbol("sun.max", "Light appearance"), mtm),
            NSImageView::imageViewWithImage(&ui::symbol("moon.stars", "Dark appearance"), mtm),
        ];
        let previews = ui::stack(mtm, true, &[]);
        previews.setSpacing(20.0);
        previews.setDistribution(NSStackViewDistribution::FillEqually);
        for (index, (image, title)) in images.iter().zip(["Light", "Dark"]).enumerate() {
            image.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            image
                .heightAnchor()
                .constraintEqualToConstant(200.0)
                .setActive(true);
            let name = ui::label(mtm, title, 15.0);
            let edit = ui::button(mtm, "Edit…", delegate, sel!(editAppearance:));
            edit.setTag(index as isize);
            let caption = ui::stack(mtm, true, &[&name, &edit]);
            caption.setDistribution(NSStackViewDistribution::EqualSpacing);
            let preview = ui::stack(mtm, false, &[image, &caption]);
            image
                .widthAnchor()
                .constraintEqualToAnchor(&preview.widthAnchor())
                .setActive(true);
            caption
                .widthAnchor()
                .constraintEqualToAnchor(&preview.widthAnchor())
                .setActive(true);
            previews.addArrangedSubview(&preview);
        }

        let schedule_title = ui::label(mtm, "Schedule", 15.0);
        let next = ui::label(mtm, "", 13.0);
        next.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let description = ui::stack(mtm, false, &[&schedule_title, &next]);
        description.setSpacing(4.0);
        let schedule = NSSwitch::new(mtm);
        unsafe {
            schedule.setTarget(Some(delegate));
            schedule.setAction(Some(sel!(toggleSchedule:)));
        }
        schedule.setToolTip(Some(&NSString::from_str("Enable schedule")));
        let schedule_row = ui::stack(mtm, true, &[&description, &schedule]);
        schedule_row.setDistribution(NSStackViewDistribution::EqualSpacing);

        let content = ui::stack(mtm, false, &[&heading, &previews, &schedule_row]);
        content.setSpacing(24.0);
        for row in [&*heading, &*previews, &*schedule_row] {
            row.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        ui::mount(
            &window.contentView().expect("window needs content view"),
            &content,
            24.0,
        );

        Self {
            window,
            mode,
            next,
            schedule,
            images,
            paths: RefCell::new([None, None]),
        }
    }

    pub fn update(&self, config: &Config, mode: Mode, next: Option<&Event>) {
        self.mode
            .setSelectedSegment(isize::from(mode == Mode::Dark));
        self.schedule.setState(if config.schedule.enabled {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        let text = if config.schedule.enabled {
            next.map(|event| format!("{} · {}", event.at.strftime("%a %H:%M"), event.mode))
                .unwrap_or_else(|| "Add an arrangement to get started".into())
        } else {
            "Switch appearance manually".into()
        };
        self.next.setStringValue(&NSString::from_str(&text));
        let mut paths = self.paths.borrow_mut();
        for (index, profile) in [&config.light, &config.dark].into_iter().enumerate() {
            if paths[index] != profile.wallpaper {
                let image = profile
                    .wallpaper
                    .as_ref()
                    .and_then(|path| {
                        NSImage::initWithContentsOfFile(
                            NSImage::alloc(),
                            &NSString::from_str(&path.to_string_lossy()),
                        )
                    })
                    .unwrap_or_else(|| {
                        ui::symbol(
                            if index == 0 { "sun.max" } else { "moon.stars" },
                            "Appearance",
                        )
                    });
                self.images[index].setImage(Some(&image));
                paths[index] = profile.wallpaper.clone();
            }
        }
    }

    pub fn show(&self) {
        ui::show(&self.window);
    }
}
