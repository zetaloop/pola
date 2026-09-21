use std::cell::RefCell;

use jiff::civil::Time;
use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::ProtocolObject,
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSDictionary, NSIndexSet, NSMutableAttributedString,
    NSObject, NSObjectProtocol, NSRange, NSString, NSTimeZone,
};

use super::{Delegate, ui};
use crate::{
    locale::tr,
    mode::Mode,
    schedule::{Rule, Weekday},
};

struct Form {
    index: Option<usize>,
    days: Retained<NSSegmentedControl>,
    time: Retained<NSDatePicker>,
    mode: Retained<NSSegmentedControl>,
    error: Retained<NSTextField>,
}

pub struct Ivars {
    owner: Weak<Delegate>,
    pages: Retained<NSTabViewController>,
    table: Retained<NSTableView>,
    enabled: Retained<NSSwitch>,
    apply: Retained<NSSwitch>,
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
            self.ivars().owner.load().map_or(0, |owner| owner.config().schedule.rules.len() as isize)
        }
    }
    unsafe impl NSControlTextEditingDelegate for Editor {}
    unsafe impl NSTableViewDelegate for Editor {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
            self.ivars().owner.load().and_then(|owner| {
            let config = owner.config();
            let rule = config.schedule.rules.get(row as usize)?;
            let time = super::locale::time(rule.time).unwrap_or_else(|error| error);
            let detail = tr!("{days}, {mode}", days = crate::locale::days(&rule.days).unwrap_or_else(|error| error), mode = rule.mode.label());
            let text = NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &NSString::from_str(&format!("{time}\n{detail}")));
            unsafe {
                let font = NSFont::preferredFontForTextStyle_options(NSFontTextStyleTitle2, &NSDictionary::new());
                text.addAttribute_value_range(NSFontAttributeName, &font, NSRange::new(0, time.encode_utf16().count()));
                text.addAttribute_value_range(NSForegroundColorAttributeName, &NSColor::secondaryLabelColor(), NSRange::new(time.encode_utf16().count() + 1, detail.encode_utf16().count()));
            }
            let cell = NSTableCellView::new(self.mtm());
            let label = NSTextField::wrappingLabelWithString(&NSString::new(), self.mtm());
            label.setAttributedStringValue(&text);
            cell.addSubview(&label);
            unsafe { cell.setTextField(Some(&label)); }
            label.setTranslatesAutoresizingMaskIntoConstraints(false);
            let margins = cell.layoutMarginsGuide();
            NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
                label.leadingAnchor().constraintEqualToAnchor(&margins.leadingAnchor()),
                label.trailingAnchor().constraintEqualToAnchor(&margins.trailingAnchor()),
                label.topAnchor().constraintEqualToAnchor(&margins.topAnchor()),
                label.bottomAnchor().constraintEqualToAnchor(&margins.bottomAnchor()),
            ]));
            Some(cell.into_super())
            })
        }
    }
    impl Editor {
        #[unsafe(method(addRule:))]
        fn add_rule(&self, _sender: &NSObject) { self.edit(None); }

        #[unsafe(method(editRule:))]
        fn edit_rule(&self, _sender: &NSObject) {
            let row = self.ivars().table.selectedRow();
            if row >= 0 { self.edit(Some(row as usize)); }
        }

        #[unsafe(method(removeRule:))]
        fn remove_rule(&self, _sender: &NSObject) {
            let index = self.ivars().form.borrow().as_ref().and_then(|form| form.index);
            if let Some(index) = index && let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                config.schedule.rules.remove(index);
                match owner.save_config(config) {
                    Ok(()) => self.close_form(),
                    Err(error) => self.form_error(&error),
                }
            }
        }

        #[unsafe(method(enabledChanged:))]
        fn enabled_changed(&self, sender: &NSSwitch) {
            if let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                config.schedule.enabled = sender.state() == NSControlStateValueOn;
                self.error(&owner.save_config(config).err().unwrap_or_default());
                self.update();
            }
        }

        #[unsafe(method(applyChanged:))]
        fn apply_changed(&self, sender: &NSSwitch) {
            if let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                config.schedule.apply_on_launch = sender.state() == NSControlStateValueOn;
                self.error(&owner.save_config(config).err().unwrap_or_default());
                self.update();
            }
        }

        #[unsafe(method(saveRule:))]
        fn save_rule(&self, _sender: &NSObject) {
            if let Some(window) = self.ivars().pages.view().window() { window.makeFirstResponder(None); }
            let change = {
                let form = self.ivars().form.borrow();
                let Some(form) = form.as_ref() else { return; };
                let days: Vec<_> = Weekday::ALL.into_iter().enumerate()
                    .filter_map(|(i, day)| form.days.isSelectedForSegment(i as isize).then_some(day)).collect();
                if days.is_empty() { form.error.setStringValue(&NSString::from_str(tr!("Choose at least one day."))); return; }
                let seconds = form.time.dateValue().timeIntervalSince1970().rem_euclid(86400.0) as i32;
                let time = Time::new((seconds / 3600) as i8, (seconds / 60 % 60) as i8, 0, 0).unwrap();
                (form.index, Rule { days, time, mode: if form.mode.selectedSegment() == 0 { Mode::Light } else { Mode::Dark } })
            };
            if let Some(owner) = self.ivars().owner.load() {
                let mut config = owner.config();
                let index = change.0.unwrap_or(config.schedule.rules.len());
                match change.0 { Some(index) => config.schedule.rules[index] = change.1, None => config.schedule.rules.push(change.1) }
                match owner.save_config(config) {
                    Ok(()) => {
                        self.close_form();
                        self.ivars().table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(index), false);
                        self.ivars().table.scrollRowToVisible(index as isize);
                    }
                    Err(error) => self.form_error(&error),
                }
            }
        }

        #[unsafe(method(cancelRule:))]
        fn cancel_rule(&self, _sender: &NSObject) { self.close_form(); }
    }
);

impl Editor {
    pub fn new(mtm: MainThreadMarker, owner: &Delegate) -> Retained<Self> {
        let table = ui::table(mtm, tr!("Schedule"));
        table.setRowSizeStyle(NSTableViewRowSizeStyle::Custom);
        table.setUsesAutomaticRowHeights(true);
        let enabled = NSSwitch::new(mtm);
        enabled.setAccessibilityLabel(Some(&NSString::from_str(tr!("Automatic switching"))));
        let apply = NSSwitch::new(mtm);
        apply.setAccessibilityLabel(Some(&NSString::from_str(tr!("Apply schedule on launch"))));
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let this = Self::alloc(mtm).set_ivars(Ivars {
            owner: Weak::new(owner),
            pages: ui::pages(mtm, &[]),
            table,
            enabled,
            apply,
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
            this.ivars().table.setAction(Some(sel!(editRule:)));
            this.ivars().enabled.setTarget(Some(&this));
            this.ivars().enabled.setAction(Some(sel!(enabledChanged:)));
            this.ivars().apply.setTarget(Some(&this));
            this.ivars().apply.setAction(Some(sel!(applyChanged:)));
        }
        let heading = ui::heading(mtm, tr!("Schedule"));
        let add = ui::button(mtm, tr!("Add rule"), &this, sel!(addRule:));
        let header = ui::stack(mtm, true, &[&heading, &add]);
        header.setDistribution(NSStackViewDistribution::EqualSpacing);
        let enabled_label = ui::label(mtm, tr!("Automatic switching"));
        let apply_label = ui::label(mtm, tr!("Apply schedule on launch"));
        let options = ui::form(
            mtm,
            &[
                [&enabled_label, &this.ivars().enabled],
                [&apply_label, &this.ivars().apply],
            ],
        );
        options
            .columnAtIndex(1)
            .setXPlacement(NSGridCellPlacement::Trailing);
        let list = NSScrollView::new(mtm);
        list.setHasVerticalScroller(true);
        list.setDrawsBackground(false);
        list.setDocumentView(Some(&this.ivars().table));
        let content = ui::stack(mtm, false, &[&header, &options, &list, &this.ivars().error]);
        for child in [&*header as &NSView, &*options, &*list, &*this.ivars().error] {
            child
                .widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        let view = NSView::new(mtm);
        ui::mount(&view, &content);
        content
            .bottomAnchor()
            .constraintEqualToAnchor(&view.layoutMarginsGuide().bottomAnchor())
            .setActive(true);
        let page = NSViewController::new(mtm);
        page.setView(&view);
        this.ivars()
            .pages
            .addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(&page));
        this.update();
        this
    }

    pub fn controller(&self) -> &NSTabViewController {
        &self.ivars().pages
    }

    pub fn update(&self) {
        if let Some(owner) = self.ivars().owner.load() {
            let schedule = owner.config().schedule;
            self.ivars().enabled.setState(isize::from(schedule.enabled));
            self.ivars()
                .apply
                .setState(isize::from(schedule.apply_on_launch));
        }
        self.ivars().table.reloadData();
    }

    fn error(&self, text: &str) {
        self.ivars().error.setStringValue(&NSString::from_str(text));
    }

    fn form_error(&self, text: &str) {
        if let Some(form) = self.ivars().form.borrow().as_ref() {
            form.error.setStringValue(&NSString::from_str(text));
        }
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
        days.setAccessibilityLabel(Some(&NSString::from_str(tr!("Days"))));
        for (i, day) in Weekday::ALL.into_iter().enumerate() {
            days.setSelected_forSegment(rule.days.contains(&day), i as isize);
        }
        let time = NSDatePicker::new(mtm);
        time.setDatePickerElements(NSDatePickerElementFlags::HourMinute);
        time.setDatePickerStyle(NSDatePickerStyle::TextFieldAndStepper);
        time.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
        time.setDateValue(&NSDate::dateWithTimeIntervalSince1970(
            f64::from(rule.time.hour()) * 3600.0 + f64::from(rule.time.minute()) * 60.0,
        ));
        time.setAccessibilityLabel(Some(&NSString::from_str(tr!("Time"))));
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
        mode.setAccessibilityLabel(Some(&NSString::from_str(tr!("Appearance"))));
        let heading = ui::heading(
            mtm,
            if index.is_some() {
                tr!("Schedule")
            } else {
                tr!("New rule")
            },
        );
        let time_label = ui::label(mtm, tr!("Time"));
        let days_label = ui::label(mtm, tr!("Days"));
        let mode_label = ui::label(mtm, tr!("Appearance"));
        let save = ui::button(mtm, tr!("Save"), self, sel!(saveRule:));
        save.setTintProminence(NSTintProminence::Primary);
        let cancel = ui::button(mtm, tr!("Cancel"), self, sel!(cancelRule:));
        let delete = ui::button(mtm, tr!("Delete rule"), self, sel!(removeRule:));
        delete.setHidden(index.is_none());
        let buttons = ui::actions(mtm, &[&delete, &cancel, &save]);
        let error = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
        error.setTextColor(Some(&NSColor::systemRedColor()));
        let content = ui::stack(
            mtm,
            false,
            &[
                &heading,
                &time_label,
                &time,
                &days_label,
                &days,
                &mode_label,
                &mode,
                &error,
                &buttons,
            ],
        );
        for child in [&*buttons as &NSView, &*error] {
            child
                .widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        let view = NSView::new(mtm);
        ui::scroll(&view, &content);
        let page = NSViewController::new(mtm);
        page.setView(&view);
        self.ivars()
            .pages
            .addTabViewItem(&NSTabViewItem::tabViewItemWithViewController(&page));
        self.ivars().pages.setSelectedTabViewItemIndex(1);
        *self.ivars().form.borrow_mut() = Some(Form {
            index,
            days,
            time,
            mode,
            error,
        });
    }

    fn close_form(&self) {
        if let Some(window) = self.ivars().pages.view().window() {
            window.makeFirstResponder(None);
        }
        self.ivars().pages.setSelectedTabViewItemIndex(0);
        let items = self.ivars().pages.tabViewItems();
        if items.len() > 1 {
            self.ivars()
                .pages
                .removeTabViewItem(&items.objectAtIndex(1));
        }
        self.ivars().form.borrow_mut().take();
        self.error("");
    }
}
