use std::{
    env, fs, io,
    os::unix::{
        ffi::OsStrExt,
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    process::Child,
    sync::{
        atomic::{AtomicIsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use pepper::{
    application::{AnyError, ApplicationEvent, ClientApplication, ServerApplication},
    client::ClientHandle,
    editor_utils::hash_bytes,
    platform::{BufPool, Key, Platform, PlatformRequest, ProcessHandle, ProcessTag, SharedBuf},
    Args,
};

mod unix_utils;
use unix_utils::{get_terminal_size, parse_terminal_keys, run, Process, RawMode};

const MAX_CLIENT_COUNT: usize = 20;
const MAX_PROCESS_COUNT: usize = 42;
const MAX_EVENT_COUNT: usize = 1 + 1 + MAX_CLIENT_COUNT + MAX_PROCESS_COUNT;
const _ASSERT_MAX_EVENT_COUNT_IS_64: [(); 64] = [(); MAX_EVENT_COUNT];
const MAX_TRIGGERED_EVENT_COUNT: usize = 32;

pub fn main() {
    let raw_mode = RawMode::enter();
    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    let mut buf = [0; 64];
    let mut keys = Vec::new();

    loop {
        use io::Read;
        let len = match stdin.read(&mut buf) {
            Ok(len) => len,
            Err(_) => return,
        };
        keys.clear();
        parse_terminal_keys(&buf[..len], &mut keys);
        for &key in &keys {
            print!("{}\r\n", key);
            if key == Key::Esc {
                return;
            }
        }
    }
    drop(raw_mode);
    //run(run_server, run_client);
}

const DEFAULT_KEVENT: libc::kevent = libc::kevent {
    ident: 0,
    filter: 0,
    flags: 0,
    fflags: 0,
    data: 0,
    udata: std::ptr::null_mut(),
};

enum Event {
    Resize,
    FlushRequest,
    Fd(RawFd),
}
impl Event {
    pub fn into_kevent(self, index: usize) -> libc::kevent {
        match self {
            Self::Resize => libc::kevent {
                ident: libc::SIGWINCH as _,
                filter: libc::EVFILT_SIGNAL,
                flags: libc::EV_ADD,
                fflags: 0,
                data: 0,
                udata: index as _,
            },
            Self::FlushRequest => libc::kevent {
                ident: 0,
                filter: libc::EVFILT_USER,
                flags: libc::EV_ADD,
                fflags: 0,
                data: 0,
                udata: index as _,
            },
            Self::Fd(fd) => libc::kevent {
                ident: fd as _,
                filter: libc::EVFILT_READ,
                flags: libc::EV_ADD,
                fflags: 0,
                data: 0,
                udata: index as _,
            },
        }
    }
}

struct KqueueEvents([libc::kevent; MAX_TRIGGERED_EVENT_COUNT]);
impl KqueueEvents {
    pub fn new() -> Self {
        Self([DEFAULT_KEVENT; MAX_TRIGGERED_EVENT_COUNT])
    }
}

struct Kqueue {
    fd: RawFd,
    tracked: [libc::kevent; MAX_EVENT_COUNT],
    tracked_len: usize,
}
impl Kqueue {
    pub fn new() -> Self {
        let fd = unsafe { libc::kqueue() };
        if fd == -1 {
            panic!("could not create kqueue");
        }
        Self {
            fd,
            tracked: [DEFAULT_KEVENT; MAX_EVENT_COUNT],
            tracked_len: 0,
        }
    }

    pub fn track(&mut self, event: Event, index: usize) {
        let insert_index = self.tracked_len;
        debug_assert!(insert_index < self.tracked.len());
        self.tracked[insert_index] = event.into_kevent(index);
        self.tracked_len += 1;
    }

    pub fn wait(&mut self) {
        let len = self.tracked_len;
        self.tracked_len = 0;

        todo!();
    }
}
impl Drop for Kqueue {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

fn run_server(listener: UnixListener) -> Result<(), AnyError> {
    use io::{Read, Write};

    const NONE_PROCESS: Option<Process> = None;

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(|| (), request_sender);
    let event_sender = ServerApplication::run(platform);

    let mut client_connections: [Option<UnixStream>; MAX_CLIENT_COUNT] = Default::default();
    let mut processes = [NONE_PROCESS; MAX_PROCESS_COUNT];
    let mut buf_pool = BufPool::default();

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(|| (), request_sender);
    let event_sender = ServerApplication::run(platform);

    let mut timeout = Some(ServerApplication::idle_duration());

    loop {
        return Ok(());
    }
}

fn run_client(args: Args, mut connection: UnixStream) {
    use io::{Read, Write};

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    let mut client_index = 0;
    match connection.read(std::slice::from_mut(&mut client_index)) {
        Ok(1) => (),
        _ => return,
    }

    let client_handle = ClientHandle::from_index(client_index as _).unwrap();
    let is_pipped = unsafe { libc::isatty(stdin.as_raw_fd()) == 0 };

    let stdout = io::stdout();
    let mut application = ClientApplication::new(client_handle, stdout.lock(), is_pipped);
    let bytes = application.init(args);
    if connection.write(bytes).is_err() {
        return;
    }

    let raw_mode;

    if is_pipped {
        raw_mode = None;
    } else {
        raw_mode = Some(RawMode::enter());

        let size = get_terminal_size();
        let bytes = application.update(Some(size), &[], &[], &[]);
        if connection.write(bytes).is_err() {
            return;
        }
    }

    //let mut keys = Vec::new();
    let mut stream_buf = [0; ClientApplication::connection_buffer_len()];
    let mut stdin_buf = [0; ClientApplication::stdin_buffer_len()];

    loop {
        break;
    }

    drop(raw_mode);
}

