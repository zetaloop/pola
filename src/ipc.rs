use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use interprocess::local_socket::{Listener, ListenerOptions, Name, Stream, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
    config::{Action, Config},
    locale::{self, tr},
    mode::Mode,
    schedule::Event,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    pub config: Config,
    pub mode: Result<Mode, String>,
    pub next: Option<Event>,
    pub launch_at_login: bool,
    pub busy: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Request {
    Subscribe,
    Save(Config),
    Select(Mode),
    Run { name: String, wait: bool },
    Shortcut(String),
    Launch(bool),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Response {
    State(State),
    Reply(Result<State, String>),
    Error(String),
}

pub enum Incoming {
    Request {
        peer: usize,
        request: Request,
        reply: mpsc::Sender<Response>,
    },
    Closed(usize),
    Error(String),
    Action {
        action: Action,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Finished {
        result: Result<(), String>,
        reply: Option<mpsc::Sender<Response>>,
    },
}

fn directory() -> io::Result<PathBuf> {
    let directory = crate::config::path()?.parent().unwrap().to_owned();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)?;
    }
    #[cfg(windows)]
    fs::create_dir_all(&directory)?;
    Ok(directory)
}

fn name() -> io::Result<Name<'static>> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::GenericFilePath;
        directory()?
            .join("daemon.sock")
            .to_fs_name::<GenericFilePath>()
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        let user = std::env::var("USERNAME").map_err(io::Error::other)?;
        format!("pola.{user}").to_ns_name::<GenericNamespaced>()
    }
}

fn lock_file(name: &str) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory()?.join(name))
}

pub fn listen(ready: bool) -> io::Result<Option<(File, Listener)>> {
    // The launcher holds the startup lock until it receives the readiness response.
    let _startup = if ready {
        None
    } else {
        let lock = lock_file("startup.lock")?;
        lock.lock()?;
        Some(lock)
    };
    let lock = lock_file("daemon.lock")?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let socket = directory()?.join("daemon.sock");
        match fs::symlink_metadata(&socket) {
            Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(socket)?,
            Ok(_) => {
                return Err(io::Error::other(tr!(
                    "daemon socket path is occupied by another file"
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let options = ListenerOptions::new().name(name()?);
    #[cfg(unix)]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    Ok(Some((lock, options.create_sync()?)))
}

pub fn serve(listener: Listener, incoming: impl Fn(Incoming) + Send + Sync + 'static) {
    let incoming = Arc::new(incoming);
    thread::spawn(move || {
        for (peer, connection) in listener.incoming().enumerate() {
            let connection = match connection {
                Ok(connection) => Arc::new(connection),
                Err(error) => {
                    incoming(Incoming::Error(error.to_string()));
                    return;
                }
            };
            let incoming = Arc::clone(&incoming);
            thread::spawn(move || {
                let (reply, responses) = mpsc::channel();
                let writer = Arc::clone(&connection);
                thread::spawn(move || {
                    for response in responses {
                        if write(&mut &*writer, &response).is_err() {
                            break;
                        }
                    }
                });
                let mut reader = BufReader::new(&*connection);
                loop {
                    match read(&mut reader) {
                        Ok(Some(request)) => incoming(Incoming::Request {
                            peer,
                            request,
                            reply: reply.clone(),
                        }),
                        Ok(None) => break,
                        Err(error) => {
                            eprintln!("IPC request: {error}");
                            break;
                        }
                    }
                }
                incoming(Incoming::Closed(peer));
            });
        }
    });
}

pub fn ready() -> io::Result<()> {
    write(&mut io::stdout(), &Ok::<(), String>(()))
}

fn connect() -> io::Result<Stream> {
    match Stream::connect(name()?) {
        Ok(stream) => return Ok(stream),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) => {}
        Err(error) => return Err(error),
    }
    let lock = lock_file("startup.lock")?;
    lock.lock()?;
    match Stream::connect(name()?) {
        Ok(stream) => return Ok(stream),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) => {}
        Err(error) => return Err(error),
    }
    let mut child = Command::new(std::env::current_exe()?)
        .args(["daemon", "--ready"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let output = child.stdout.take().unwrap();
    thread::spawn(move || {
        let _ = child.wait();
    });
    let result: Result<(), String> = read(&mut BufReader::new(output))?
        .ok_or_else(|| io::Error::other(tr!("daemon exited before opening its connection")))?;
    result.map_err(io::Error::other)?;
    Stream::connect(name()?)
}

struct Shared {
    state: Option<State>,
    pending: Option<mpsc::Sender<Result<(), String>>>,
    closed: Option<String>,
}

pub struct Client {
    stream: Mutex<Arc<Stream>>,
    shared: Arc<Mutex<Shared>>,
}

impl Client {
    pub fn connect(changed: impl Fn(Option<String>) + Send + 'static) -> Result<Self, String> {
        Self::new(connect().map_err(|error| error.to_string())?, changed)
    }

    fn new(
        stream: Stream,
        changed: impl Fn(Option<String>) + Send + 'static,
    ) -> Result<Self, String> {
        let stream = Arc::new(stream);
        let shared = Arc::new(Mutex::new(Shared {
            state: None,
            pending: None,
            closed: None,
        }));
        let client = Self {
            stream: Mutex::new(Arc::clone(&stream)),
            shared: Arc::clone(&shared),
        };
        thread::spawn(move || {
            let mut reader = BufReader::new(&*stream);
            loop {
                let response = match read(&mut reader) {
                    Ok(Some(response)) => response,
                    result => {
                        let error = match result {
                            Err(error) => error.to_string(),
                            _ => tr!("Connection to pola background process closed").into(),
                        };
                        let mut shared = shared.lock().unwrap();
                        shared.closed = Some(error.clone());
                        if let Some(pending) = shared.pending.take() {
                            _ = pending.send(Err(error.clone()));
                        }
                        drop(shared);
                        changed(Some(error));
                        return;
                    }
                };
                match response {
                    Response::State(state) => {
                        locale::set(state.config.language);
                        shared.lock().unwrap().state = Some(state);
                        changed(None);
                    }
                    Response::Reply(result) => {
                        let mut shared = shared.lock().unwrap();
                        let result = result.map(|state| {
                            locale::set(state.config.language);
                            shared.state = Some(state);
                        });
                        if let Some(pending) = shared.pending.take() {
                            _ = pending.send(result);
                        }
                    }
                    Response::Error(error) => changed(Some(error)),
                }
            }
        });
        client.request(Request::Subscribe)?;
        Ok(client)
    }

    pub fn state(&self) -> State {
        self.shared.lock().unwrap().state.as_ref().unwrap().clone()
    }

    pub fn request(&self, request: Request) -> Result<(), String> {
        let stream = self.stream.lock().unwrap();
        let (pending, result) = mpsc::channel();
        {
            let mut shared = self.shared.lock().unwrap();
            if let Some(error) = &shared.closed {
                return Err(error.clone());
            }
            shared.pending = Some(pending);
        }
        if let Err(error) = write(&mut &**stream, &request) {
            let mut shared = self.shared.lock().unwrap();
            shared.closed = Some(error.to_string());
            shared.pending.take();
            return Err(error.to_string());
        }
        result.recv().map_err(|error| error.to_string())?
    }
}

pub fn write(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn read<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> io::Result<Option<T>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&line)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use interprocess::local_socket::GenericNamespaced;

    #[test]
    fn notifications_and_replies_share_connection() {
        let directory = tempfile::tempdir().unwrap();
        let name = format!(
            "pola-{}",
            directory.path().file_name().unwrap().to_str().unwrap()
        )
        .to_ns_name::<GenericNamespaced>()
        .unwrap();
        let listener = ListenerOptions::new()
            .name(name.clone())
            .create_sync()
            .unwrap();
        let server = thread::spawn(move || {
            let connection = listener.accept().unwrap();
            let mut reader = BufReader::new(&connection);
            let mut state = State {
                config: Config::default(),
                mode: Ok(Mode::Light),
                next: None,
                launch_at_login: false,
                busy: false,
            };
            assert!(matches!(
                read(&mut reader).unwrap(),
                Some(Request::Subscribe)
            ));
            write(&mut &connection, &Response::Reply(Ok(state.clone()))).unwrap();
            assert!(matches!(
                read(&mut reader).unwrap(),
                Some(Request::Select(Mode::Dark))
            ));
            write(&mut &connection, &Response::State(state.clone())).unwrap();
            state.mode = Ok(Mode::Dark);
            write(&mut &connection, &Response::Reply(Ok(state))).unwrap();
            assert!(matches!(
                read(&mut reader).unwrap(),
                Some(Request::Launch(true))
            ));
            write(&mut &connection, &Response::Reply(Err("rejected".into()))).unwrap();
            assert!(matches!(
                read(&mut reader).unwrap(),
                Some(Request::Select(Mode::Light))
            ));
        });
        let (changed, notifications) = mpsc::channel();
        let client = Client::new(Stream::connect(name).unwrap(), move |error| {
            _ = changed.send(error);
        })
        .unwrap();
        client.request(Request::Select(Mode::Dark)).unwrap();
        assert_eq!(client.state().mode, Ok(Mode::Dark));
        assert_eq!(notifications.recv().unwrap(), None);
        assert_eq!(
            client.request(Request::Launch(true)),
            Err("rejected".into())
        );
        assert!(client.request(Request::Select(Mode::Light)).is_err());
        server.join().unwrap();
    }
}
