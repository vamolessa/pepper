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

    let mut kqueue = Kqueue::new();

    'main_loop: loop {
        kqueue.track(Event::Fd(stdin.as_raw_fd()), 0);

        print!("waiting for events...\r\n");
        let events = kqueue.wait(None);
        for event_index in events {
            match event_index {
                Ok(0) => {
                    use io::Read;
                    let len = stdin.read(&mut buf).unwrap();
                    keys.clear();
                    parse_terminal_keys(&buf[..len], &mut keys);
                    for &key in &keys {
                        print!("{}\r\n", key);
                        if key == Key::Esc {
                            break 'main_loop;
                        }
                    }
                }
                Ok(_) => unreachable!(),
                Err(()) => {
                    panic!("ops something bad happened")
                }
            };
        }
    }

    drop(raw_mode);
    //run(run_server, run_client);
}

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
                flags: libc::EV_ADD | libc::EV_ENABLE,
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

struct Kqueue {
    fd: RawFd,
    tracked: [libc::kevent; MAX_EVENT_COUNT],
    tracked_len: usize,
    triggered: [libc::kevent; MAX_TRIGGERED_EVENT_COUNT],
}
impl Kqueue {
    pub fn new() -> Self {
        const DEFAULT_KEVENT: libc::kevent = libc::kevent {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        };

        let fd = unsafe { libc::kqueue() };
        if fd == -1 {
            panic!("could not create kqueue");
        }
        Self {
            fd,
            tracked: [DEFAULT_KEVENT; MAX_EVENT_COUNT],
            tracked_len: 0,
            triggered: [DEFAULT_KEVENT; MAX_TRIGGERED_EVENT_COUNT],
        }
    }

    pub fn track(&mut self, event: Event, index: usize) {
        let insert_index = self.tracked_len;
        debug_assert!(insert_index < self.tracked.len());
        self.tracked[insert_index] = event.into_kevent(index);
        self.tracked_len += 1;
    }

    pub fn wait(
        &mut self,
        timeout: Option<Duration>,
    ) -> impl ExactSizeIterator<Item = Result<usize, ()>> {
        let mut timespec = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let timeout = match timeout {
            Some(duration) => {
                timespec.tv_nsec = duration.as_nanos() as _;
                &timespec as *const _
            }
            None => std::ptr::null(),
        };

        let tracked = &self.tracked[..self.tracked_len];
        self.tracked_len = 0;

        let len = unsafe {
            libc::kevent(
                self.fd,
                tracked.as_ptr(),
                tracked.len() as _,
                self.triggered.as_mut_ptr(),
                self.triggered.len() as _,
                timeout,
            )
        };
        if len == -1 {
            panic!("could not wait for events");
        }

        self.triggered[..len as usize].iter().map(|e| {
            if e.flags & libc::EV_ERROR != 0 {
                Err(())
            } else {
                Ok(e.udata as _)
            }
        })
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
