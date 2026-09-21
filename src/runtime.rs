use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    sync::mpsc::Sender,
};

use jiff::Zoned;

#[cfg(target_os = "macos")]
use crate::macos::system;
#[cfg(target_os = "windows")]
use crate::windows::system;
use crate::{
    config::Config,
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
            mode: system::mode(),
            launch_at_login: system::launch_at_login(),
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
        }
    }

    pub fn save(&self, config: Config) -> Result<(), String> {
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

    pub fn select(&self, mode: Mode) -> Result<(), String> {
        if !system::mode().is_ok_and(|current| current == mode) {
            system::set_mode(mode)?;
        }
        self.observe()
    }

    pub fn toggle(&self) -> Result<(), String> {
        self.select(system::mode()?.toggle())
    }

    pub fn observe(&self) -> Result<(), String> {
        let mode = system::mode()?;
        if self.applied.replace(Some(mode)) == Some(mode) {
            return Ok(());
        }
        let profile = self.config.borrow().profile(mode).clone();
        let mut errors = Vec::new();
        if let Some(path) = &profile.wallpaper
            && let Err(error) = system::set_wallpaper(path)
        {
            errors.push(format!("Wallpaper: {error}"));
        }
        for command in &profile.commands {
            if let Err(error) = command.run() {
                errors.push(format!("{}: {error}", command.program));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
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
