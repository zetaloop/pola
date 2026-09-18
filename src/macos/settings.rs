use std::{cell::RefCell, str::FromStr};

use jiff::civil::Time;
use objc2::{AnyThread, MainThreadOnly, rc::Retained, sel};
use objc2_app_kit::{
    NSAlert, NSBackingStoreType, NSButton, NSControlStateValueOff, NSControlStateValueOn,
    NSPopUpButton, NSScrollView, NSTabView, NSTabViewItem, NSTextField, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

use crate::{
    config::Config,
    mode::Mode,
    schedule::{Rule, Weekday},
};

use super::Delegate;

const WIDTH: f64 = 720.0;
const HEIGHT: f64 = 540.0;
const CONTENT_HEIGHT: f64 = 452.0;
const ROW_HEIGHT: f64 = 34.0;

struct ScheduleRow {
    view: Retained<NSView>,
    days: Vec<Retained<NSButton>>,
    time: Retained<NSTextField>,
    mode: Retained<NSPopUpButton>,
    remove: Retained<NSButton>,
}

pub struct Settings {
    window: Retained<NSWindow>,
    tabs: Retained<NSTabView>,
    launch_at_login: Retained<NSButton>,
    schedule_enabled: Retained<NSButton>,
    apply_on_launch: Retained<NSButton>,
    shortcut: Retained<NSTextField>,
    schedule: Retained<NSView>,
    rules: RefCell<Vec<ScheduleRow>>,
}

impl Settings {
    pub fn new(mtm: MainThreadMarker, delegate: &Delegate) -> Self {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("pola"));
        unsafe { window.setReleasedWhenClosed(false) };
        window.center();

        let content = window
            .contentView()
            .expect("settings window needs content view");
        let tabs = NSTabView::initWithFrame(
            NSTabView::alloc(mtm),
            NSRect::new(NSPoint::new(20.0, 64.0), NSSize::new(680.0, 450.0)),
        );
        content.addSubview(&tabs);

        let general = tab(&tabs, mtm, "General");
        let schedule_tab = tab(&tabs, mtm, "Schedule");

        let launch_at_login = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str("Launch at login"),
                None,
                None,
                mtm,
            )
        };
        launch_at_login.setFrame(rect(24.0, 356.0, 260.0, 24.0));
        general.addSubview(&launch_at_login);

        let schedule_enabled = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str("Enable schedule"),
                None,
                None,
                mtm,
            )
        };
        schedule_enabled.setFrame(rect(24.0, 318.0, 260.0, 24.0));
        general.addSubview(&schedule_enabled);

        let apply_on_launch = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str("Apply schedule on launch"),
                None,
                None,
                mtm,
            )
        };
        apply_on_launch.setFrame(rect(24.0, 280.0, 260.0, 24.0));
        general.addSubview(&apply_on_launch);

        let shortcut_label = NSTextField::labelWithString(&NSString::from_str("Shortcut"), mtm);
        shortcut_label.setFrame(rect(24.0, 230.0, 110.0, 24.0));
        general.addSubview(&shortcut_label);

        let shortcut = NSTextField::textFieldWithString(&NSString::new(), mtm);
        shortcut.setFrame(rect(140.0, 228.0, 260.0, 24.0));
        general.addSubview(&shortcut);

        let schedule_scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(16.0, 48.0, 640.0, 344.0));
        schedule_scroll.setHasVerticalScroller(true);
        let schedule =
            NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 620.0, CONTENT_HEIGHT));
        schedule_scroll.setDocumentView(Some(&schedule));
        schedule_tab.addSubview(&schedule_scroll);

        let add_rule = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Add"),
                Some(delegate),
                Some(sel!(addSchedule:)),
                mtm,
            )
        };
        add_rule.setFrame(rect(16.0, 8.0, 90.0, 30.0));
        schedule_tab.addSubview(&add_rule);

        let save = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Save"),
                Some(delegate),
                Some(sel!(saveSettings:)),
                mtm,
            )
        };
        save.setFrame(rect(610.0, 18.0, 90.0, 32.0));
        content.addSubview(&save);

        Self {
            window,
            tabs,
            launch_at_login,
            schedule_enabled,
            apply_on_launch,
            shortcut,
            schedule,
            rules: RefCell::new(Vec::new()),
        }
    }

    pub fn load(&self, config: &Config, delegate: &Delegate) {
        self.launch_at_login.setState(if super::launch_at_login() {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.schedule_enabled.setState(if config.schedule.enabled {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.apply_on_launch
            .setState(if config.schedule.apply_on_launch {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        self.shortcut
            .setStringValue(&NSString::from_str(&config.shortcut));

        self.clear_rules();
        for rule in &config.schedule.rules {
            self.add_rule(delegate, Some(rule));
        }
    }

    pub fn config(&self, current: &Config) -> Result<Config, Box<dyn std::error::Error>> {
        let shortcut = self.shortcut.stringValue().to_string();

        let rules = self
            .rules
            .borrow()
            .iter()
            .map(rule_value)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Config {
            shortcut,
            schedule: crate::schedule::Schedule {
                enabled: self.schedule_enabled.state() == NSControlStateValueOn,
                apply_on_launch: self.apply_on_launch.state() == NSControlStateValueOn,
                rules,
            },
            ..current.clone()
        })
    }

    pub fn add_rule(&self, delegate: &Delegate, rule: Option<&Rule>) {
        let mtm = self.window.mtm();
        let view = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 620.0, ROW_HEIGHT));

        let mut days = Vec::with_capacity(7);
        for (index, title) in ["M", "T", "W", "T", "F", "S", "S"].into_iter().enumerate() {
            let button = unsafe {
                NSButton::checkboxWithTitle_target_action(
                    &NSString::from_str(title),
                    None,
                    None,
                    mtm,
                )
            };
            button.setFrame(rect(8.0 + index as f64 * 42.0, 5.0, 40.0, 24.0));
            if rule.is_some_and(|rule| rule.days.contains(&WEEKDAYS[index])) {
                button.setState(NSControlStateValueOn);
            }
            view.addSubview(&button);
            days.push(button);
        }

        let time = NSTextField::textFieldWithString(
            &NSString::from_str(
                &rule
                    .map(|rule| rule.time.strftime("%H:%M").to_string())
                    .unwrap_or_else(|| "18:00".into()),
            ),
            mtm,
        );
        time.setFrame(rect(306.0, 5.0, 74.0, 24.0));
        view.addSubview(&time);

        let mode = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(390.0, 3.0, 100.0, 28.0),
            false,
        );
        mode.addItemWithTitle(&NSString::from_str("Light"));
        mode.addItemWithTitle(&NSString::from_str("Dark"));
        mode.selectItemWithTitle(&NSString::from_str(match rule.map(|rule| rule.mode) {
            Some(Mode::Dark) => "Dark",
            _ => "Light",
        }));
        view.addSubview(&mode);

        let remove = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Remove"),
                Some(delegate),
                Some(sel!(removeSchedule:)),
                mtm,
            )
        };
        remove.setFrame(rect(506.0, 3.0, 96.0, 28.0));
        view.addSubview(&remove);

        self.rules.borrow_mut().push(ScheduleRow {
            view,
            days,
            time,
            mode,
            remove,
        });
        self.layout_rules();
    }

    pub fn remove_rule(&self, index: usize) {
        let mut rows = self.rules.borrow_mut();
        if index >= rows.len() {
            return;
        }
        rows.remove(index).view.removeFromSuperview();
        drop(rows);
        self.layout_rules();
    }

    pub fn select_page(&self, index: isize) {
        self.tabs.selectTabViewItemAtIndex(index);
    }

    pub fn is_visible(&self) -> bool {
        self.window.isVisible()
    }

    pub fn launch_at_login(&self) -> bool {
        self.launch_at_login.state() == NSControlStateValueOn
    }

    pub fn show(&self) {
        self.window.makeKeyAndOrderFront(None);
        objc2_app_kit::NSApplication::sharedApplication(self.window.mtm()).activate();
    }

    pub fn show_error(&self, message: &str) {
        let alert = NSAlert::new(self.window.mtm());
        alert.setMessageText(&NSString::from_str("Could not save settings"));
        alert.setInformativeText(&NSString::from_str(message));
        alert.runModal();
    }

    fn clear_rules(&self) {
        for row in self.rules.borrow_mut().drain(..) {
            row.view.removeFromSuperview();
        }
    }

    fn layout_rules(&self) {
        let rows = self.rules.borrow();
        let height = (rows.len() as f64 * ROW_HEIGHT).max(CONTENT_HEIGHT);
        self.schedule.setFrameSize(NSSize::new(620.0, height));

        for (index, row) in rows.iter().enumerate() {
            row.view.setFrameOrigin(NSPoint::new(
                0.0,
                height - (index as f64 + 1.0) * ROW_HEIGHT,
            ));
            row.remove.setTag(index as isize);
        }
    }
}

fn rule_value(row: &ScheduleRow) -> Result<Rule, Box<dyn std::error::Error>> {
    let days = row
        .days
        .iter()
        .zip(WEEKDAYS)
        .filter_map(|(button, day)| (button.state() == NSControlStateValueOn).then_some(day))
        .collect::<Vec<_>>();

    if days.is_empty() {
        return Err("schedule rule needs at least one day".into());
    }

    let time = Time::from_str(&row.time.stringValue().to_string())?;
    let mode = if row.mode.indexOfSelectedItem() == 1 {
        Mode::Dark
    } else {
        Mode::Light
    };

    Ok(Rule { days, time, mode })
}

fn tab(tabs: &NSTabView, mtm: MainThreadMarker, label: &str) -> Retained<NSView> {
    let item = unsafe { NSTabViewItem::initWithIdentifier(NSTabViewItem::alloc(), None) };
    item.setLabel(&NSString::from_str(label));
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(664.0, 410.0)),
    );
    item.setView(Some(&view));
    tabs.addTabViewItem(&item);
    view
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];
