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
        parse_terminal_keys(&buf[..len], &mut keys);
        for &key in &keys {
            println!("{}", key);
            if key == Key::Esc {
                return;
            }
        }
    }
    drop(raw_mode);
    //run(run_server, run_client);
}

struct Kqueue(RawFd);
impl Kqueue {
    pub fn new() -> Self {
        let fd = unsafe { libc::kqueue() };
        if fd == -1 {
            panic!("could not create kqueue");
        }
        Self(fd)
    }
    
    pub fn wait(&self) {
        //
    }
}
impl Drop for Kqueue {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

fn run_server(listener: UnixListener) -> Result<(), AnyError> {
    use io::{Read, Write};

    const NONE_PROCESS: Option<Process> = None;

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(
        || (),
        request_sender,
    );
    let event_sender = ServerApplication::run(platform);

    let mut client_connections: [Option<UnixStream>; MAX_CLIENT_COUNT] = Default::default();
    let mut processes = [NONE_PROCESS; MAX_PROCESS_COUNT];
    let mut buf_pool = BufPool::default();

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(
        || (),
        request_sender,
    );
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

