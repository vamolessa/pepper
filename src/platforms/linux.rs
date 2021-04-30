use std::{
    env, fs, io,
    os::unix::{
        ffi::OsStrExt,
        io::IntoRawFd,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::{Child, ChildStdin},
    sync::{
        atomic::{AtomicI32, Ordering},
        mpsc,
    },
    time::Duration,
};

use libc::{
    c_int, c_void, close, epoll_create1, eventfd, fork, sigaction, sigemptyset, siginfo_t,
    SA_SIGINFO, SIGINT, EFD_CLOEXEC, EFD_NONBLOCK, read, write,
};

use pepper::{
    application::{AnyError, ApplicationEvent, ClientApplication, ServerApplication},
    client::ClientHandle,
    editor_utils::hash_bytes,
    platform::{
        BufPool, ExclusiveBuf, Key, Platform, PlatformRequest, ProcessHandle, ProcessTag,
        SharedBuf,
    },
    Args,
};

pub fn main() {
    println!("hello from linux");

    let args = Args::parse();

    let mut hash_buf = [0u8; 16];
    let session_name = match args.session {
        Some(ref name) => name.as_str(),
        None => {
            use io::Write;

            let current_dir = env::current_dir().expect("could not retrieve the current directory");
            let current_dir_bytes = current_dir.as_os_str().as_bytes().iter().cloned();
            let current_directory_hash = hash_bytes(current_dir_bytes);
            let mut cursor = io::Cursor::new(&mut hash_buf[..]);
            write!(&mut cursor, "{:x}", current_directory_hash).unwrap();
            let len = cursor.position() as usize;
            std::str::from_utf8(&hash_buf[..len]).unwrap()
        }
    };

    let mut stream_path = String::new();
    stream_path.push_str("/tmp/");
    stream_path.push_str(env!("CARGO_PKG_NAME"));
    stream_path.push('/');
    stream_path.push_str(session_name);

    if args.print_session {
        print!("{}", stream_path);
        return;
    }

    set_ctrlc_handler();

    let stream_path = Path::new(&stream_path);

    if args.force_server {
        let _ = run_server(stream_path);
        fs::remove_file(stream_path);
        return;
    }
    return;

    match UnixStream::connect(stream_path) {
        Ok(stream) => run_client(args, stream),
        Err(_) => match unsafe { fork() } {
            -1 => panic!("could not start server"),
            0 => loop {
                match UnixStream::connect(stream_path) {
                    Ok(stream) => {
                        run_client(args, stream);
                        break;
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            },
            _ => {
                let _ = run_server(stream_path);
                fs::remove_file(stream_path);
            }
        },
    }
}

fn set_ctrlc_handler() {
    unsafe extern "system" fn handler(_: c_int, _: *const siginfo_t, _: *const c_void) {}

    let mut action = sigaction {
        sa_sigaction: handler as _,
        sa_mask: unsafe { std::mem::zeroed() },
        sa_flags: SA_SIGINFO,
        sa_restorer: None,
    };

    let result = unsafe { sigemptyset(&mut action.sa_mask as _) };
    if result != 0 {
        panic!("could not set ctrl handler");
    }

    let result = unsafe { sigaction(SIGINT, &action as _, std::ptr::null_mut()) };
    if result != 0 {
        panic!("could not set ctrl handler");
    }
}

fn notify_event(fd: c_int) {
    let mut buf = 1u64.to_ne_bytes();
    let result = unsafe { write(fd, buf.as_mut_ptr() as _, buf.len() as _) };
    if result != buf.len() as _ {
        panic!("could not read event");
    }
}

struct Event(c_int);
impl Event {
    pub fn new() -> Self {
        let fd = unsafe { eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK) };
        if fd == -1 {
            panic!("could not create event");
        }
        Self(fd)
    }

    pub fn notify(&self) {
        notify_event(self.0);
    }
    
    pub fn read(&self) {
        let mut buf = 1u64.to_ne_bytes();
        let result = unsafe { read(self.0, buf.as_mut_ptr() as _, buf.len() as _) };
        if result != buf.len() as _ {
            panic!("could not notify event");
        }
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe { close(self.0) };
    }
}

fn read_from_clipboard(text: &mut String) -> bool {
    // TODO: read from clipboard
    text.clear();
    true
}

fn write_to_clipboard(text: &str) {
    // TODO write to clipboard
}

fn run_server(stream_path: &Path) -> Result<(), AnyError> {
    static NEW_REQUEST_FD: AtomicI32 = AtomicI32::new(-1);

    if let Some(dir) = stream_path.parent() {
        if !dir.exists() {
            let _ = fs::create_dir(dir);
        }
    }

    let _ = fs::remove_file(stream_path);
    let listener =
        UnixListener::bind(stream_path).expect("could not start unix domain socket server");

    let mut buf_pool = BufPool::default();

    let new_request_event = Event::new();
    NEW_REQUEST_FD.store(new_request_event.0 as _, Ordering::Relaxed);

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(
        read_from_clipboard,
        write_to_clipboard,
        || (), // TODO: write to NEW_REQUEST_FD
        request_sender,
    );

    let event_sender = match ServerApplication::run(platform) {
        Some(sender) => sender,
        None => return Ok(()),
    };

    let mut timeout = Some(ServerApplication::idle_duration());

    loop {
        // TODO: main loop
    }
}

fn run_client(args: Args, stream: UnixStream) {
    // TODO
}