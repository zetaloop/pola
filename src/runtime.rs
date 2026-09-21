use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    sync::mpsc::{self, Sender},
    thread,
};

use jiff::Zoned;

#[cfg(target_os = "macos")]
use crate::macos::{daemon, system};
#[cfg(target_os = "windows")]
use crate::windows::{daemon, system};
use crate::{
    config::{Action, Config, Profile},
    ipc::{Incoming, Request, Response, State},
    mode::Mode,
    schedule::Event,
    shortcut::Shortcut,
};

pub struct Runtime {
    pub config: RefCell<Config>,
    pub shortcut: RefCell<Shortcut>,
    pub next: RefCell<Option<Event>>,
    applied: Cell<Option<Mode>>,
    busy: Cell<bool>,
    peers: RefCell<BTreeMap<usize, Sender<Response>>>,
    recording: Cell<Option<usize>>,
    error: RefCell<Option<String>>,
}

impl Runtime {
    pub fn new(config: Config) -> Self {
        Self {
            config: RefCell::new(config),
            shortcut: RefCell::new(Shortcut::default()),
            next: RefCell::new(None),
            applied: Cell::new(None),
            busy: Cell::new(false),
            peers: RefCell::new(BTreeMap::new()),
            recording: Cell::new(None),
            error: RefCell::new(None),
        }
    }

    pub fn start(&self) {
        let result = self
            .shortcut
            .borrow_mut()
            .register(&self.config.borrow().shortcut);
        if let Err(error) = result {
            self.report(error.to_string());
        }
        let mode = {
            let config = self.config.borrow();
            config
                .schedule
                .apply_on_launch
                .then(|| config.schedule.current(&Zoned::now()))
                .flatten()
        };
        let result = match mode {
            Some(mode) => self.select(mode),
            None => self.observe(),
        };
        if let Err(error) = result {
            self.report(error);
        }
    }

    pub fn state(&self) -> State {
        let config = self.config.borrow().clone();
        State {
            next: config.schedule.next(&Zoned::now()),
            config,
            mode: self.mode(),
            launch_at_login: system::launch_at_login(),
            busy: self.busy.get(),
        }
    }

    pub fn receive(&self, incoming: Incoming) {
        match incoming {
            Incoming::Request {
                peer,
                request,
                reply,
            } => {
                let result = match request {
                    Request::Subscribe => {
                        self.peers.borrow_mut().insert(peer, reply.clone());
                        if let Some(error) = self.error.borrow_mut().take() {
                            _ = reply.send(Response::Error(error));
                        }
                        Ok(())
                    }
                    Request::Save(config) => self.save(config),
                    Request::Select(mode) => self.select(mode),
                    Request::Run { name, wait } => {
                        let result = self.run(&name, wait.then(|| reply.clone()));
                        if wait && result.is_ok() {
                            return;
                        }
                        result
                    }
                    Request::Shortcut(text) => {
                        let result = self
                            .shortcut
                            .borrow_mut()
                            .register(&text)
                            .map_err(|error| error.to_string());
                        if result.is_ok() {
                            self.recording.set(text.is_empty().then_some(peer));
                        }
                        result
                    }
                    Request::Launch(enabled) => system::set_launch_at_login(enabled),
                };
                _ = reply.send(Response::Reply(result.map(|()| self.state())));
            }
            Incoming::Closed(peer) => {
                self.peers.borrow_mut().remove(&peer);
                if self.recording.get() == Some(peer) {
                    self.recording.set(None);
                    let result = self
                        .shortcut
                        .borrow_mut()
                        .register(&self.config.borrow().shortcut);
                    if let Err(error) = result {
                        self.report(error.to_string());
                    }
                }
            }
            Incoming::Error(error) => self.report(error),
            Incoming::Action { action, reply } => {
                let result = match action {
                    Action::Color { mode } => self.select(mode),
                    #[cfg(target_os = "windows")]
                    Action::Theme { path } => {
                        let result = system::set_theme(&path);
                        if let Err(error) = self.observe() {
                            self.report(error);
                        }
                        result
                    }
                    Action::Wallpaper { path } => system::set_wallpaper(&path),
                    Action::Command(_) => unreachable!("commands run on the execution thread"),
                };
                _ = reply.send(result);
            }
            Incoming::Finished { result, reply } => {
                self.busy.set(false);
                if let Some(reply) = reply {
                    _ = reply.send(Response::Reply(result.map(|()| self.state())));
                } else if let Err(error) = result {
                    self.report(error);
                }
            }
        }
    }

    pub fn save(&self, config: Config) -> Result<(), String> {
        config.validate().map_err(|error| error.to_string())?;
        self.shortcut
            .borrow_mut()
            .register(&config.shortcut)
            .map_err(|error| error.to_string())?;
        if let Err(error) = config.save() {
            if let Err(restore) = self
                .shortcut
                .borrow_mut()
                .register(&self.config.borrow().shortcut)
            {
                return Err(format!("{error}\nShortcut: {restore}"));
            }
            return Err(error.to_string());
        }
        self.recording.set(None);
        *self.config.borrow_mut() = config;
        Ok(())
    }

    fn mode(&self) -> Result<Mode, String> {
        self.applied.get().map(Ok).unwrap_or_else(system::mode)
    }

    pub fn select(&self, mode: Mode) -> Result<(), String> {
        if !self.mode().is_ok_and(|current| current == mode) {
            system::set_mode(mode)?;
        }
        self.changed(mode)
    }

    pub fn toggle(&self) -> Result<(), String> {
        self.select(self.mode()?.toggle())
    }

    pub fn observe(&self) -> Result<(), String> {
        self.changed(system::mode()?)
    }

    fn changed(&self, mode: Mode) -> Result<(), String> {
        let changed = self.applied.replace(Some(mode)) != Some(mode);
        if !changed || self.busy.get() {
            return Ok(());
        }
        let profiles = self
            .config
            .borrow()
            .profiles
            .iter()
            .filter(|profile| profile.when.contains(&mode))
            .cloned()
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            return Ok(());
        }
        self.execute(profiles, None)
    }

    pub fn run(&self, name: &str, reply: Option<Sender<Response>>) -> Result<(), String> {
        let profile = self
            .config
            .borrow()
            .profile(name)
            .cloned()
            .ok_or_else(|| format!("Configuration {name:?} was not found."))?;
        self.execute(vec![profile], reply)
    }

    fn execute(
        &self,
        profiles: Vec<Profile>,
        reply: Option<Sender<Response>>,
    ) -> Result<(), String> {
        if self.busy.replace(true) {
            return Err("A configuration is already running.".into());
        }
        if let Err(error) = thread::Builder::new().spawn(move || {
            let mut errors = Vec::new();
            'profiles: for profile in profiles {
                for action in profile.actions {
                    let result = match action {
                        Action::Command(command) => command
                            .run()
                            .map_err(|error| format!("{}: {error}", command.program)),
                        action => {
                            let (reply, result) = mpsc::channel();
                            daemon::receive(Incoming::Action { action, reply });
                            match result.recv() {
                                Ok(result) => result,
                                Err(error) => {
                                    errors.push(error.to_string());
                                    break 'profiles;
                                }
                            }
                        }
                    };
                    if let Err(error) = result {
                        errors.push(format!("{}: {error}", profile.name));
                    }
                }
            }
            let result = if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("\n"))
            };
            daemon::receive(Incoming::Finished { result, reply });
        }) {
            self.busy.set(false);
            return Err(error.to_string());
        }
        Ok(())
    }

    pub fn scheduled(&self) -> Result<(), String> {
        let mode = self.config.borrow().schedule.current(&Zoned::now());
        match mode {
            Some(mode) => self.select(mode),
            None => Ok(()),
        }
    }

    pub fn resume(&self) -> Result<(), String> {
        let missed = self
            .next
            .borrow()
            .as_ref()
            .is_some_and(|event| event.at.timestamp() <= Zoned::now().timestamp());
        if missed { self.scheduled() } else { Ok(()) }
    }

    pub fn schedule(&self) -> Option<Event> {
        let next = self.config.borrow().schedule.next(&Zoned::now());
        *self.next.borrow_mut() = next.clone();
        self.publish();
        next
    }

    pub fn report(&self, error: String) {
        eprintln!("{error}");
        if self.peers.borrow().is_empty() {
            *self.error.borrow_mut() = Some(error.clone());
        }
        self.broadcast(Response::Error(error));
    }

    fn publish(&self) {
        self.broadcast(Response::State(self.state()));
    }

    fn broadcast(&self, response: Response) {
        self.peers
            .borrow_mut()
            .retain(|_, peer| peer.send(response.clone()).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_during_execution_are_consumed() {
        let runtime = Runtime::new(Config {
            profiles: vec![Profile {
                name: "Night".into(),
                when: [Mode::Dark].into(),
                ..Profile::default()
            }],
            ..Config::default()
        });
        runtime.applied.set(Some(Mode::Light));
        runtime.busy.set(true);
        runtime.changed(Mode::Dark).unwrap();
        assert_eq!(runtime.applied.get(), Some(Mode::Dark));
        assert!(runtime.run("Night", None).is_err());
        runtime.receive(Incoming::Finished {
            result: Ok(()),
            reply: None,
        });
        runtime.changed(Mode::Dark).unwrap();
        assert!(!runtime.busy.get());
    }
}
