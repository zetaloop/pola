use std::{cell::RefCell, path::PathBuf, str::FromStr};

use global_hotkey::hotkey::HotKey;
use jiff::civil::Time;
use objc2::{AnyThread, MainThreadOnly, rc::Retained, sel};
use objc2_app_kit::{
    NSAlert, NSBackingStoreType, NSButton, NSControlStateValueOff, NSControlStateValueOn,
    NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSScrollView, NSTabView, NSTabViewItem,
    NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

use crate::{
    config::{Command, Config, Profile},
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

struct ArgRow {
    field: Retained<NSTextField>,
    remove: Retained<NSButton>,
}

struct CommandRow {
    view: Retained<NSView>,
    program: Retained<NSTextField>,
    args: Vec<ArgRow>,
    add_arg: Retained<NSButton>,
    remove: Retained<NSButton>,
}

struct ProfileControls {
    wallpaper: Retained<NSTextField>,
    commands: Retained<NSView>,
    rows: RefCell<Vec<CommandRow>>,
}

pub struct Settings {
    window: Retained<NSWindow>,
    schedule_enabled: Retained<NSButton>,
    apply_on_launch: Retained<NSButton>,
    shortcut: Retained<NSTextField>,
    schedule: Retained<NSView>,
    rules: RefCell<Vec<ScheduleRow>>,
    light: ProfileControls,
    dark: ProfileControls,
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
        let light_tab = tab(&tabs, mtm, "Light");
        let dark_tab = tab(&tabs, mtm, "Dark");

        let schedule_enabled = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str("Enable schedule"),
                None,
                None,
                mtm,
            )
        };
        schedule_enabled.setFrame(rect(24.0, 356.0, 260.0, 24.0));
        general.addSubview(&schedule_enabled);

        let apply_on_launch = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str("Apply schedule on launch"),
                None,
                None,
                mtm,
            )
        };
        apply_on_launch.setFrame(rect(24.0, 318.0, 260.0, 24.0));
        general.addSubview(&apply_on_launch);

        let shortcut_label = NSTextField::labelWithString(&NSString::from_str("Shortcut"), mtm);
        shortcut_label.setFrame(rect(24.0, 268.0, 110.0, 24.0));
        general.addSubview(&shortcut_label);

        let shortcut = NSTextField::textFieldWithString(&NSString::new(), mtm);
        shortcut.setFrame(rect(140.0, 266.0, 260.0, 24.0));
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

        let light = profile_controls(mtm, delegate, &light_tab, Mode::Light);
        let dark = profile_controls(mtm, delegate, &dark_tab, Mode::Dark);

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
            schedule_enabled,
            apply_on_launch,
            shortcut,
            schedule,
            rules: RefCell::new(Vec::new()),
            light,
            dark,
        }
    }

    pub fn load(&self, config: &Config, delegate: &Delegate) {
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
            .setStringValue(&NSString::from_str(&config.general.shortcut));

        self.clear_rules();
        for rule in &config.schedule.rules {
            self.add_rule(delegate, Some(rule));
        }

        self.load_profile(delegate, Mode::Light, &config.light);
        self.load_profile(delegate, Mode::Dark, &config.dark);
    }

    pub fn config(&self) -> Result<Config, Box<dyn std::error::Error>> {
        let shortcut = self.shortcut.stringValue().to_string();
        HotKey::from_str(&shortcut)?;

        let rules = self
            .rules
            .borrow()
            .iter()
            .map(rule_value)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Config {
            general: crate::config::General { shortcut },
            schedule: crate::schedule::Schedule {
                enabled: self.schedule_enabled.state() == NSControlStateValueOn,
                apply_on_launch: self.apply_on_launch.state() == NSControlStateValueOn,
                rules,
            },
            light: self.profile_value(Mode::Light),
            dark: self.profile_value(Mode::Dark),
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

    pub fn browse(&self, mode: Mode) {
        let panel = NSOpenPanel::openPanel(self.window.mtm());
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);

        if panel.runModal() == NSModalResponseOK
            && let Some(url) = panel.URL()
            && let Some(path) = url.path()
        {
            self.profile(mode).wallpaper.setStringValue(&path);
        }
    }

    pub fn add_command(&self, delegate: &Delegate, mode: Mode, command: Option<&Command>) {
        let mtm = self.window.mtm();
        let controls = self.profile(mode);
        let view = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 620.0, 68.0));

        let program = NSTextField::textFieldWithString(
            &NSString::from_str(command.map_or("", |command| &command.program)),
            mtm,
        );
        program.setPlaceholderString(Some(&NSString::from_str("Program")));
        program.setFrame(rect(8.0, 36.0, 330.0, 24.0));
        view.addSubview(&program);

        let add_arg = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Add argument"),
                Some(delegate),
                Some(match mode {
                    Mode::Light => sel!(addLightArgument:),
                    Mode::Dark => sel!(addDarkArgument:),
                }),
                mtm,
            )
        };
        add_arg.setFrame(rect(344.0, 34.0, 122.0, 28.0));
        view.addSubview(&add_arg);

        let remove = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Remove"),
                Some(delegate),
                Some(match mode {
                    Mode::Light => sel!(removeLightCommand:),
                    Mode::Dark => sel!(removeDarkCommand:),
                }),
                mtm,
            )
        };
        remove.setFrame(rect(474.0, 34.0, 96.0, 28.0));
        view.addSubview(&remove);

        let args = command
            .map(|command| {
                command
                    .args
                    .iter()
                    .map(|argument| arg_row(mtm, delegate, mode, &view, argument))
                    .collect()
            })
            .unwrap_or_default();

        controls.rows.borrow_mut().push(CommandRow {
            view,
            program,
            args,
            add_arg,
            remove,
        });
        self.layout_commands(mode);
    }

    pub fn remove_command(&self, mode: Mode, index: usize) {
        let controls = self.profile(mode);
        let mut rows = controls.rows.borrow_mut();
        if index >= rows.len() {
            return;
        }
        rows.remove(index).view.removeFromSuperview();
        drop(rows);
        self.layout_commands(mode);
    }

    pub fn add_argument(&self, delegate: &Delegate, mode: Mode, command: usize) {
        let mtm = self.window.mtm();
        let controls = self.profile(mode);
        let mut rows = controls.rows.borrow_mut();
        let Some(row) = rows.get_mut(command) else {
            return;
        };
        row.args.push(arg_row(mtm, delegate, mode, &row.view, ""));
        drop(rows);
        self.layout_commands(mode);
    }

    pub fn remove_argument(&self, mode: Mode, command: usize, argument: usize) {
        let controls = self.profile(mode);
        let mut rows = controls.rows.borrow_mut();
        let Some(row) = rows.get_mut(command) else {
            return;
        };
        if argument >= row.args.len() {
            return;
        }
        let arg = row.args.remove(argument);
        arg.field.removeFromSuperview();
        arg.remove.removeFromSuperview();
        drop(rows);
        self.layout_commands(mode);
    }

    pub fn is_visible(&self) -> bool {
        self.window.isVisible()
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

    fn load_profile(&self, delegate: &Delegate, mode: Mode, profile: &Profile) {
        let controls = self.profile(mode);
        controls.wallpaper.setStringValue(&NSString::from_str(
            &profile
                .wallpaper
                .as_ref()
                .map(|path| path.to_string_lossy())
                .unwrap_or_default(),
        ));

        for row in controls.rows.borrow_mut().drain(..) {
            row.view.removeFromSuperview();
        }
        for command in &profile.commands {
            self.add_command(delegate, mode, Some(command));
        }
        self.layout_commands(mode);
    }

    fn profile_value(&self, mode: Mode) -> Profile {
        let controls = self.profile(mode);
        let wallpaper = controls.wallpaper.stringValue().to_string();

        Profile {
            wallpaper: (!wallpaper.is_empty()).then(|| PathBuf::from(wallpaper)),
            commands: controls
                .rows
                .borrow()
                .iter()
                .map(|row| Command {
                    program: row.program.stringValue().to_string(),
                    args: row
                        .args
                        .iter()
                        .map(|arg| arg.field.stringValue().to_string())
                        .collect(),
                })
                .filter(|command| !command.program.is_empty())
                .collect(),
        }
    }

    fn profile(&self, mode: Mode) -> &ProfileControls {
        match mode {
            Mode::Light => &self.light,
            Mode::Dark => &self.dark,
        }
    }

    fn layout_commands(&self, mode: Mode) {
        let controls = self.profile(mode);
        let mut rows = controls.rows.borrow_mut();
        let mut y = 0.0;

        for (command_index, row) in rows.iter_mut().enumerate().rev() {
            let command_height = 68.0 + row.args.len() as f64 * ROW_HEIGHT;
            row.view.setFrameSize(NSSize::new(620.0, command_height));
            row.view.setFrameOrigin(NSPoint::new(0.0, y));
            row.program
                .setFrame(rect(8.0, command_height - 32.0, 330.0, 24.0));
            row.add_arg
                .setFrame(rect(344.0, command_height - 34.0, 122.0, 28.0));
            row.remove
                .setFrame(rect(474.0, command_height - 34.0, 96.0, 28.0));
            row.add_arg.setTag(command_index as isize);
            row.remove.setTag(command_index as isize);

            for (argument_index, arg) in row.args.iter().enumerate() {
                let arg_y = command_height - 66.0 - argument_index as f64 * ROW_HEIGHT;
                arg.field.setFrame(rect(28.0, arg_y, 410.0, 24.0));
                arg.remove.setFrame(rect(448.0, arg_y - 2.0, 96.0, 28.0));
                arg.remove
                    .setTag(pack(command_index, argument_index) as isize);
            }

            y += command_height + 8.0;
        }

        controls
            .commands
            .setFrameSize(NSSize::new(620.0, y.max(CONTENT_HEIGHT - 92.0)));
    }
}

fn profile_controls(
    mtm: MainThreadMarker,
    delegate: &Delegate,
    tab: &NSView,
    mode: Mode,
) -> ProfileControls {
    let wallpaper_label = NSTextField::labelWithString(&NSString::from_str("Wallpaper"), mtm);
    wallpaper_label.setFrame(rect(16.0, 366.0, 90.0, 24.0));
    tab.addSubview(&wallpaper_label);

    let wallpaper = NSTextField::textFieldWithString(&NSString::new(), mtm);
    wallpaper.setFrame(rect(108.0, 364.0, 426.0, 24.0));
    tab.addSubview(&wallpaper);

    let browse = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Choose…"),
            Some(delegate),
            Some(match mode {
                Mode::Light => sel!(browseLight:),
                Mode::Dark => sel!(browseDark:),
            }),
            mtm,
        )
    };
    browse.setFrame(rect(542.0, 362.0, 100.0, 28.0));
    tab.addSubview(&browse);

    let commands_label = NSTextField::labelWithString(&NSString::from_str("Commands"), mtm);
    commands_label.setFrame(rect(16.0, 326.0, 100.0, 24.0));
    tab.addSubview(&commands_label);

    let add = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Add"),
            Some(delegate),
            Some(match mode {
                Mode::Light => sel!(addLightCommand:),
                Mode::Dark => sel!(addDarkCommand:),
            }),
            mtm,
        )
    };
    add.setFrame(rect(552.0, 320.0, 90.0, 30.0));
    tab.addSubview(&add);

    let scroll =
        NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(16.0, 8.0, 626.0, 304.0));
    scroll.setHasVerticalScroller(true);

    let commands = NSView::initWithFrame(
        NSView::alloc(mtm),
        rect(0.0, 0.0, 620.0, CONTENT_HEIGHT - 92.0),
    );
    scroll.setDocumentView(Some(&commands));
    tab.addSubview(&scroll);

    ProfileControls {
        wallpaper,
        commands,
        rows: RefCell::new(Vec::new()),
    }
}

fn arg_row(
    mtm: MainThreadMarker,
    delegate: &Delegate,
    mode: Mode,
    parent: &NSView,
    argument: &str,
) -> ArgRow {
    let field = NSTextField::textFieldWithString(&NSString::from_str(argument), mtm);
    field.setPlaceholderString(Some(&NSString::from_str("Argument")));
    parent.addSubview(&field);

    let remove = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Remove"),
            Some(delegate),
            Some(match mode {
                Mode::Light => sel!(removeLightArgument:),
                Mode::Dark => sel!(removeDarkArgument:),
            }),
            mtm,
        )
    };
    parent.addSubview(&remove);

    ArgRow { field, remove }
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

fn pack(command: usize, argument: usize) -> usize {
    command << 16 | argument
}

pub fn unpack(tag: isize) -> (usize, usize) {
    let tag = tag as usize;
    (tag >> 16, tag & 0xffff)
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
