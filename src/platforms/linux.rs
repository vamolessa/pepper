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
        atomic::{AtomicIsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use libc::{
    c_int, c_void, close, epoll_create1, eventfd, fork, read, sigaction, sigemptyset,
    siginfo_t, tcflag_t, tcgetattr, tcsetattr, termios, write, ECHO, EFD_NONBLOCK, SA_SIGINFO,
    SIGINT, STDIN_FILENO, TCSAFLUSH, ICANON,
};

use pepper::{
    application::{AnyError, ApplicationEvent, ClientApplication, ServerApplication},
    client::ClientHandle,
    editor_utils::hash_bytes,
    platform::{
        BufPool, ExclusiveBuf, Key, Platform, PlatformRequest, ProcessHandle, ProcessTag, SharedBuf,
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

    // temp
    let (stream, _) = UnixStream::pair().unwrap();
    run_client(args, stream);
    return;
    // temp

    if args.force_server {
        let _ = run_server(stream_path);
        let _ = fs::remove_file(stream_path);
        return;
    }

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
                let _ = fs::remove_file(stream_path);
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

    let result = unsafe { sigemptyset(&mut action.sa_mask) };
    if result != 0 {
        panic!("could not set ctrl handler");
    }

    let result = unsafe { sigaction(SIGINT, &action, std::ptr::null_mut()) };
    if result != 0 {
        panic!("could not set ctrl handler");
    }
}

struct RawMode {
    original: termios,
}
impl RawMode {
    pub fn enter() -> Self {
        let original = unsafe {
            let mut original = std::mem::zeroed();
            tcgetattr(STDIN_FILENO, &mut original);
            let mut new = original.clone();
            new.c_lflag &= !(ECHO | ICANON);
            tcsetattr(STDIN_FILENO, TCSAFLUSH, &new);
            original
        };
        Self { original }
    }
}
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { tcsetattr(STDIN_FILENO, TCSAFLUSH, &self.original) };
    }
}

fn write_to_event_fd(fd: c_int) {
    let mut buf = 1u64.to_ne_bytes();
    loop {
        let result = unsafe { write(fd, buf.as_mut_ptr() as _, buf.len() as _) };
        if result == -1 {
            if let io::ErrorKind::WouldBlock = io::Error::last_os_error().kind() {
                std::thread::yield_now();
                continue;
            }
        }
        if result != buf.len() as _ {
            panic!("could not write to event fd");
        }
    }
}

struct EventFd(c_int);
impl EventFd {
    pub fn new() -> Self {
        // TODO: maybe no need for NONBLOCK if we use epoll level triggered
        let fd = unsafe { eventfd(0, EFD_NONBLOCK) };
        if fd == -1 {
            panic!("could not create event");
        }
        Self(fd)
    }

    pub fn write(&self) {
        write_to_event_fd(self.0);
    }

    pub fn read(&self) {
        let mut buf = [0; 8];
        let result = unsafe { read(self.0, buf.as_mut_ptr() as _, buf.len() as _) };
        if result != buf.len() as _ {
            panic!("could not read from event fd");
        }
    }
}
impl Drop for EventFd {
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
    static NEW_REQUEST_FD: AtomicIsize = AtomicIsize::new(-1);

    if let Some(dir) = stream_path.parent() {
        if !dir.exists() {
            let _ = fs::create_dir(dir);
        }
    }

    let _ = fs::remove_file(stream_path);
    let listener =
        UnixListener::bind(stream_path).expect("could not start unix domain socket server");

    let mut buf_pool = BufPool::default();

    let new_request_event = EventFd::new();
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
    println!("client");
    
    let raw_mode = RawMode::enter();

    let mut buf = [0];
    loop {
        let result = unsafe { read(STDIN_FILENO, buf.as_mut_ptr() as _, buf.len() as _) };
        if result != 1 {
            break;
        }
        println!("{}", buf[0]);
        if buf[0] == b'q' {
            break;
        }
    }
    
    drop(raw_mode);
}
