use std::cell::RefCell;

use crate::locale::tr;

use jiff::civil::Time;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSObject, NSObjectProtocol, NSString, NSTimeZone,
};

use super::{Delegate, ui};
use crate::{
    mode::Mode,
    schedule::{Rule, Weekday},
};

struct Form {
    window: Retained<NSWindow>,
    index: Option<usize>,
    days: Retained<NSSegmentedControl>,
    time: Retained<NSDatePicker>,
    mode: Retained<NSSegmentedControl>,
    error: Retained<NSTextField>,
}

pub struct Ivars {
    owner: Weak<Delegate>,
    view: Retained<NSView>,
    table: Retained<NSTableView>,
    enabled: Retained<NSSwitch>,
    error: Retained<NSTextField>,
    form: RefCell<Option<Form>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Editor;

    unsafe impl NSObjectProtocol for Editor {}
    unsafe impl NSTableViewDataSource for Editor {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _table: &NSTableView) -> isize {
            self.ivars()
                .owner
                .load()
                .map_or(0, |owner| owner.config().schedule.rules.len() as isize)
        }
    }
    unsafe impl NSControlTextEditingDelegate for Editor {}
    unsafe impl NSTableViewDelegate for Editor {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(
            &self,
            _table: &NSTableView,
            column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.ivars().owner.load().map(|owner| {
                let config = owner.config();
                let rule = &config.schedule.rules[row as usize];
                let text = match column.map(|c| c.identifier().to_string()).as_deref() {
                    Some("days") => crate::locale::days(&rule.days).unwrap_or_else(|error| error),
                    Some("time") => super::locale::time(rule.time).unwrap_or_else(|error| error),
                    _ => rule.mode.label().into(),
                };
                ui::cell(self.mtm(), &text, None).into_super()
            })
        }
    }
    impl Editor {
        #[unsafe(method(addRule:))]
        fn add_rule(&self, _sender: &NSObject) {
            self.edit(None);
        }
        #[unsafe(method(editRule:))]
        fn edit_rule(&self, _sender: &NSObject) {
            let row = self.ivars().table.selectedRow();
            if row >= 0 {
                self.edit(Some(row as usize));
            }
        }
        #[unsafe(method(removeRule:))]
        fn remove_rule(&self, _sender: &NSObject) {
            let row = self.ivars().table.selectedRow();
            if row >= 0
                && let Some(owner) = self.ivars().owner.load()
            {
                let mut config = owner.config();
                config.schedule.rules.remove(row as usize);
                match owner.save_config(config) {
                    Ok(()) => self.error(""),
                    Err(error) => self.error(&error),
                }
                self.update();
            }
        }
        #[unsafe(method(enabledChanged:))]
        fn enabled_changed(&self, sender: &NSSwitch) {
            if let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                config.schedule.enabled = sender.state() == NSControlStateValueOn;
                match owner.save_config(config) {
                    Ok(()) => self.error(""),
                    Err(error) => self.error(&error),
                }
                self.update();
            }
        }
        #[unsafe(method(saveRule:))]
        fn save_rule(&self, _sender: &NSObject) {
            let change = {
                let form = self.ivars().form.borrow();
                let Some(form) = form.as_ref() else {
                    return;
                };
                form.window.makeFirstResponder(None);
                let days: Vec<_> = Weekday::ALL
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, day)| form.days.isSelectedForSegment(i as isize).then_some(day))
                    .collect();
                if days.is_empty() {
                    form.error
                        .setStringValue(&NSString::from_str(tr!("Choose at least one day.")));
                    return;
                }
                let seconds = form
                    .time
                    .dateValue()
                    .timeIntervalSince1970()
                    .rem_euclid(86400.0) as i32;
                let time = Time::new((seconds / 3600) as i8, (seconds / 60 % 60) as i8, 0, 0).unwrap();
                (
                    form.index,
                    Rule {
                        days,
                        time,
                        mode: if form.mode.selectedSegment() == 0 {
                            Mode::Light
                        } else {
                            Mode::Dark
                        },
                    },
                )
            };
            if let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                match change.0 {
                    Some(i) => config.schedule.rules[i] = change.1,
                    None => config.schedule.rules.push(change.1),
                }
                match owner.save_config(config) {
                    Ok(()) => {
                        self.close_form();
                        self.error("");
                        self.update();
                    }
                    Err(error) => {
                        if let Some(form) = self.ivars().form.borrow().as_ref() {
                            form.error.setStringValue(&NSString::from_str(&error));
                        }
                    }
                }
            }
        }
        #[unsafe(method(cancelRule:))]
        fn cancel_rule(&self, _sender: &NSObject) {
            self.close_form();
        }
    }
);

impl Editor {
    pub fn new(mtm: MainThreadMarker, owner: &Delegate) -> Retained<Self> {
        let view = NSView::new(mtm);
        let table = NSTableView::new(mtm);
        table.setRowSizeStyle(NSTableViewRowSizeStyle::Default);
        table.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        table.setStyle(NSTableViewStyle::Inset);
        for (name, title, width) in [
            ("days", tr!("Days"), 250.0),
            ("time", tr!("Time"), 100.0),
            ("mode", tr!("Appearance"), 110.0),
        ] {
            let column = NSTableColumn::initWithIdentifier(
                NSTableColumn::alloc(mtm),
                &NSString::from_str(name),
            );
            column.setTitle(&NSString::from_str(title));
            column.setWidth(width);
            table.addTableColumn(&column);
        }
        let enabled = NSSwitch::new(mtm);
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            view,
            table,
            enabled,
            error,
            form: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe {
            this.ivars()
                .table
                .setDataSource(Some(ProtocolObject::from_ref(&*this)));
            this.ivars()
                .table
                .setDelegate(Some(ProtocolObject::from_ref(&*this)));
            this.ivars().table.setTarget(Some(&this));
            this.ivars().table.setDoubleAction(Some(sel!(editRule:)));
            this.ivars().enabled.setTarget(Some(&this));
            this.ivars().enabled.setAction(Some(sel!(enabledChanged:)));
        }
        let heading = ui::heading(mtm, tr!("Schedule"));
        let header = ui::stack(mtm, true, &[&heading, &this.ivars().enabled]);
        header.setDistribution(NSStackViewDistribution::EqualSpacing);
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&this.ivars().table));
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(230.0)
            .setActive(true);
        let add = ui::button(mtm, tr!("Add arrangement"), &this, sel!(addRule:));
        let edit = ui::button(mtm, tr!("Edit…"), &this, sel!(editRule:));
        let remove = ui::button(mtm, tr!("Remove"), &this, sel!(removeRule:));
        let actions = ui::stack(mtm, true, &[&add, &edit, &remove]);
        let content = ui::stack(
            mtm,
            false,
            &[&header, &scroll, &actions, &this.ivars().error],
        );
        for child in [&*header as &NSView, &*scroll, &*this.ivars().error] {
            child
                .widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        ui::mount(&this.ivars().view, &content);
        this.update();
        this
    }

    pub fn view(&self) -> &NSView {
        &self.ivars().view
    }
    pub fn update(&self) {
        if let Some(owner) = self.ivars().owner.load() {
            self.ivars()
                .enabled
                .setState(isize::from(owner.config().schedule.enabled));
        }
        self.ivars().table.reloadData();
    }
    fn error(&self, text: &str) {
        self.ivars().error.setStringValue(&NSString::from_str(text));
    }

    fn edit(&self, index: Option<usize>) {
        let Some(owner) = self.ivars().owner.load() else {
            return;
        };
        let names = match super::locale::weekdays() {
            Ok(names) => names,
            Err(error) => {
                self.error(&error);
                return;
            }
        };
        let config = owner.config();
        let rule = index
            .map(|index| config.schedule.rules[index].clone())
            .unwrap_or_else(|| Rule {
                days: Weekday::ALL[..5].to_vec(),
                time: Time::new(18, 0, 0, 0).unwrap(),
                mode: Mode::Dark,
            });
        let mtm = self.mtm();
        let window = ui::window(mtm, tr!("Arrangement"), 560.0, 300.0);
        let days = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(
                    &names
                        .iter()
                        .map(|name| NSString::from_str(name))
                        .collect::<Vec<_>>(),
                ),
                NSSegmentSwitchTracking::SelectAny,
                None,
                None,
                mtm,
            )
        };
        days.setControlSize(NSControlSize::Large);
        for (i, day) in Weekday::ALL.into_iter().enumerate() {
            days.setSelected_forSegment(rule.days.contains(&day), i as isize);
        }
        let time = NSDatePicker::new(mtm);
        time.setControlSize(NSControlSize::Large);
        time.setDatePickerElements(NSDatePickerElementFlags::HourMinute);
        time.setDatePickerStyle(NSDatePickerStyle::TextFieldAndStepper);
        time.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
        time.setDateValue(&NSDate::dateWithTimeIntervalSince1970(
            f64::from(rule.time.hour()) * 3600.0 + f64::from(rule.time.minute()) * 60.0,
        ));
        let mode = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&[
                    NSString::from_str(tr!("Light")),
                    NSString::from_str(tr!("Dark")),
                ]),
                NSSegmentSwitchTracking::SelectOne,
                None,
                None,
                mtm,
            )
        };
        mode.setSelectedSegment(isize::from(rule.mode == Mode::Dark));
        mode.setControlSize(NSControlSize::Large);
        let days_label = ui::label(mtm, tr!("Days"));
        let time_label = ui::label(mtm, tr!("Time"));
        let mode_label = ui::label(mtm, tr!("Appearance"));
        let form = ui::form(
            mtm,
            &[
                [&days_label, &days],
                [&time_label, &time],
                [&mode_label, &mode],
            ],
        );
        form.columnAtIndex(1)
            .setXPlacement(NSGridCellPlacement::Leading);
        let save = ui::button(mtm, tr!("Save"), self, sel!(saveRule:));
        save.setControlSize(NSControlSize::Large);
        save.setKeyEquivalent(&NSString::from_str("\r"));
        let cancel = ui::button(mtm, tr!("Cancel"), self, sel!(cancelRule:));
        cancel.setControlSize(NSControlSize::Large);
        cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        let actions = ui::actions(mtm, &[&cancel, &save]);
        let error = ui::label(mtm, "");
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let content = ui::stack(mtm, false, &[&form, &error, &actions]);
        form.widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        actions
            .widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
        ui::mount(&window.contentView().unwrap(), &content);
        self.view()
            .window()
            .unwrap()
            .beginSheet_completionHandler(&window, None);
        *self.ivars().form.borrow_mut() = Some(Form {
            window,
            index,
            days,
            time,
            mode,
            error,
        });
    }

    fn close_form(&self) {
        if let Some(form) = self.ivars().form.borrow_mut().take() {
            self.view().window().unwrap().endSheet(&form.window);
        }
    }
}
