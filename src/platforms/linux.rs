use std::{
    env, fs, io,
    os::unix::{
        ffi::OsStrExt,
        io::{IntoRawFd},
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
    c_int, c_void, close, epoll_create1, eventfd, fork, ioctl, read, sigaction, sigemptyset,
    siginfo_t, tcflag_t, tcgetattr, tcsetattr, termios, winsize, write, BRKINT, CS8, ECHO,
    EFD_NONBLOCK, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST, SA_SIGINFO, SIGINT,
    SIGWINCH, STDIN_FILENO, STDOUT_FILENO, TCSAFLUSH, TIOCGWINSZ, VMIN, VTIME,
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

    let stream_path = Path::new(&stream_path);

    // temp
    let (stream, _) = UnixStream::pair().unwrap();
    let _ = run_client(args, stream);
    return;
    // temp

    if args.force_server {
        let _ = run_server(stream_path);
        let _ = fs::remove_file(stream_path);
        return;
    }

    match UnixStream::connect(stream_path) {
        Ok(stream) => {
            let _ = run_client(args, stream);
        }
        Err(_) => match unsafe { fork() } {
            -1 => panic!("could not start server"),
            0 => loop {
                match UnixStream::connect(stream_path) {
                    Ok(stream) => {
                        let _ = run_client(args, stream);
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
    static NEW_REQUEST_EVENT_FD: AtomicIsize = AtomicIsize::new(-1);

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
    NEW_REQUEST_EVENT_FD.store(new_request_event.0 as _, Ordering::Relaxed);

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(
        read_from_clipboard,
        write_to_clipboard,
        || write_to_event_fd(NEW_REQUEST_EVENT_FD.load(Ordering::Relaxed) as _),
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

fn set_signal_handler(
    signal: c_int,
    handler: unsafe extern "system" fn(c_int, *const siginfo_t, *const c_void),
) -> bool {
    let mut action = sigaction {
        sa_sigaction: handler as _,
        sa_mask: unsafe { std::mem::zeroed() },
        sa_flags: SA_SIGINFO,
        sa_restorer: None,
    };

    let result = unsafe { sigemptyset(&mut action.sa_mask) };
    if result != 0 {
        return false;
    }

    let result = unsafe { sigaction(signal, &action, std::ptr::null_mut()) };
    if result != 0 {
        return false;
    }

    true
}

fn set_ctrlc_handler() {
    unsafe extern "system" fn handler(_: c_int, _: *const siginfo_t, _: *const c_void) {}
    if !set_signal_handler(SIGINT, handler) {
        panic!("could not set ctrl handler");
    }
}

static RESIZE_EVENT_FD: AtomicIsize = AtomicIsize::new(-1);

fn set_window_size_changed_handler() {
    unsafe extern "system" fn handler(signal: c_int, _: *const siginfo_t, _: *const c_void) {
        if signal == SIGWINCH {
            write_to_event_fd(RESIZE_EVENT_FD.load(Ordering::Relaxed) as _);
        }
    }
    if !set_signal_handler(SIGWINCH, handler) {
        panic!("could not set window size changed handler");
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
            new.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
            new.c_oflag &= !OPOST;
            new.c_cflag |= CS8;
            new.c_lflag &= !(ECHO | ICANON | ISIG | IEXTEN);
            new.c_cc[VMIN] = 0;
            new.c_cc[VTIME] = 1;
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

fn run_client(args: Args, stream: UnixStream) {
    set_ctrlc_handler();
    set_window_size_changed_handler();

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    
    print!("client\r\n");

    let raw_mode = RawMode::enter();
    let (width, height) = get_console_size();
    print!("console size: {}, {}\r\n", width, height);

    let mut keys = Vec::new();

    'main_loop: loop {
        keys.clear();
        if !read_console_keys(&mut stdin, &mut keys) {
            print!("cabo keys\r\n");
            break;
        }
        for &key in &keys {
            print!("key: {}\r\n", key);
            if key == Key::Esc {
                break 'main_loop;
            }
        }
    }

    drop(raw_mode);
}

fn get_console_size() -> (usize, usize) {
    let mut size: winsize = unsafe { std::mem::zeroed() };
    let result = unsafe { ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut size as *mut winsize) };
    if result == -1 || size.ws_col == 0 {
        panic!("could not get console size");
    }

    (size.ws_col as _, size.ws_row as _)
}

fn read_console_keys<R>(reader: &mut R, keys: &mut Vec<Key>) -> bool
where
    R: io::Read,
{
    let mut buf = [0; 64];
    let len = match reader.read(&mut buf) {
        Ok(0) => return false,
        Ok(len) => len,
        Err(error) => match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => return true,
            _ => return false,
        },
    };
    let mut buf = &buf[..len];

    loop {
        let (key, rest) = match buf {
            &[] => break true,
            &[0x1b, b'[', b'5', b'~', ref rest @ ..] => (Key::PageUp, rest),
            &[0x1b, b'[', b'6', b'~', ref rest @ ..] => (Key::PageDown, rest),
            &[0x1b, b'[', b'A', ref rest @ ..] => (Key::Up, rest),
            &[0x1b, b'[', b'B', ref rest @ ..] => (Key::Down, rest),
            &[0x1b, b'[', b'C', ref rest @ ..] => (Key::Right, rest),
            &[0x1b, b'[', b'D', ref rest @ ..] => (Key::Left, rest),
            &[0x1b, b'[', b'1', b'~', ref rest @ ..]
            | &[0x1b, b'[', b'7', b'~', ref rest @ ..]
            | &[0x1b, b'[', b'H', ref rest @ ..]
            | &[0x1b, b'O', b'H', ref rest @ ..] => (Key::Home, rest),
            &[0x1b, b'[', b'4', b'~', ref rest @ ..]
            | &[0x1b, b'[', b'8', b'~', ref rest @ ..]
            | &[0x1b, b'[', b'F', ref rest @ ..]
            | &[0x1b, b'O', b'F', ref rest @ ..] => (Key::End, rest),
            &[0x1b, b'[', b'3', b'~', ref rest @ ..] => (Key::Delete, rest),
            &[0x1b, ref rest @ ..] => (Key::Esc, rest),
            &[0x8, ref rest @ ..] => (Key::Backspace, rest),
            &[b'\n', ref rest @ ..] => (Key::Enter, rest),
            &[b'\t', ref rest @ ..] => (Key::Tab, rest),
            &[0x7f, ref rest @ ..] => (Key::Delete, rest),
            &[b @ 0b0..=0b11111, ref rest @ ..] => {
                let byte = b | 0b01100000;
                (Key::Ctrl(byte as _), rest)
            }
            _ => match buf.iter().position(|b| b.is_ascii()).unwrap_or(buf.len()) {
                0 => (Key::Char(buf[0] as _), &buf[1..]),
                len => {
                    let (c, rest) = buf.split_at(len);
                    match std::str::from_utf8(c) {
                        Ok(s) => match s.chars().next() {
                            Some(c) => (Key::Char(c), rest),
                            None => (Key::None, rest),
                        },
                        Err(_) => (Key::None, rest),
                    }
                }
            },
        };
        buf = rest;
        keys.push(key);
    }
}
