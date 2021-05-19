use std::{
    env, fs, io,
    os::unix::{
        ffi::OsStrExt,
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::Child,
    time::Duration,
};

use pepper::{
    application::{AnyError, ClientApplication},
    editor_utils::hash_bytes,
    platform::{BufPool, Key, ProcessTag, SharedBuf},
    Args,
};

pub fn run(
    server_fn: fn(Args, UnixListener) -> Result<(), AnyError>,
    client_fn: fn(Args, UnixStream),
) {
    let args = Args::parse();

    let mut session_path = String::new();
    session_path.push_str("/tmp/");
    session_path.push_str(env!("CARGO_PKG_NAME"));
    session_path.push('/');

    match args.session {
        Some(ref name) => session_path.push_str(name),
        None => {
            use io::Write;

            let current_dir = env::current_dir().expect("could not retrieve the current directory");
            let current_dir_bytes = current_dir.as_os_str().as_bytes().iter().cloned();
            let current_directory_hash = hash_bytes(current_dir_bytes);

            let mut hash_buf = [0u8; 16];
            let mut cursor = io::Cursor::new(&mut hash_buf[..]);
            write!(&mut cursor, "{:x}", current_directory_hash).unwrap();
            let len = cursor.position() as usize;
            let name = std::str::from_utf8(&hash_buf[..len]).unwrap();
            session_path.push_str(name);
        }
    }

    if args.print_session {
        print!("{}", session_path);
        return;
    }

    let session_path = Path::new(&session_path);

    fn start_server(session_path: &Path) -> UnixListener {
        if let Some(dir) = session_path.parent() {
            if !dir.exists() {
                let _ = fs::create_dir(dir);
            }
        }

        let _ = fs::remove_file(session_path);
        UnixListener::bind(session_path).expect("could not start unix domain socket server")
    }

    if args.server {
        let _ = server_fn(args, start_server(session_path));
        let _ = fs::remove_file(session_path);
    } else {
        match UnixStream::connect(session_path) {
            Ok(stream) => client_fn(args, stream),
            Err(_) => match unsafe { libc::fork() } {
                -1 => panic!("could not start server"),
                0 => {
                    let _ = server_fn(args, start_server(session_path));
                    let _ = fs::remove_file(session_path);
                }
                _ => loop {
                    match UnixStream::connect(session_path) {
                        Ok(stream) => {
                            client_fn(args, stream);
                            break;
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(100)),
                    }
                },
            },
        }
    }
}

pub struct RawMode {
    original: libc::termios,
}
impl RawMode {
    pub fn enter() -> Self {
        let original = unsafe {
            let mut original = std::mem::zeroed();
            libc::tcgetattr(libc::STDIN_FILENO, &mut original);
            let mut new = original.clone();
            new.c_iflag &= !(libc::IGNBRK
                | libc::BRKINT
                | libc::PARMRK
                | libc::ISTRIP
                | libc::INLCR
                | libc::IGNCR
                | libc::ICRNL
                | libc::IXON);
            new.c_oflag &= !libc::OPOST;
            new.c_cflag &= !(libc::CSIZE | libc::PARENB);
            new.c_cflag |= libc::CS8;
            new.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
            new.c_lflag |= libc::NOFLSH;
            new.c_cc[libc::VMIN] = 0;
            new.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &new);
            original
        };
        Self { original }
    }

    pub fn backspace_code(&self) -> u8 {
        self.original.c_cc[libc::VERASE]
    }
}
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.original) };
    }
}

pub fn is_pipped() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) == 0 }
}

pub fn read(fd: RawFd, buf: &mut [u8]) -> Result<usize, ()> {
    let len = unsafe { libc::read(fd, buf.as_mut_ptr() as _, buf.len() as _) };
    if len >= 0 {
        Ok(len as _)
    } else {
        Err(())
    }
}

pub fn read_from_connection(
    connection: &mut UnixStream,
    buf_pool: &mut BufPool,
    len: usize,
) -> Result<SharedBuf, ()> {
    use io::Read;
    let mut buf = buf_pool.acquire();
    let write = buf.write_with_len(len);
    match connection.read(write) {
        Ok(len) => {
            write.truncate(len);
            let buf = buf.share();
            buf_pool.release(buf.clone());
            Ok(buf)
        }
        Err(_) => {
            buf_pool.release(buf.share());
            Err(())
        }
    }
}

pub struct Process {
    alive: bool,
    child: Child,
    tag: ProcessTag,
    buf_len: usize,
}
impl Process {
    pub fn new(child: Child, tag: ProcessTag, buf_len: usize) -> Self {
        Self {
            alive: true,
            child,
            tag,
            buf_len,
        }
    }

    pub fn tag(&self) -> ProcessTag {
        self.tag
    }

    pub fn try_as_raw_fd(&self) -> Option<RawFd> {
        self.child.stdout.as_ref().map(|s| s.as_raw_fd())
    }

    pub fn read(&mut self, buf_pool: &mut BufPool) -> Result<Option<SharedBuf>, ()> {
        use io::Read;
        match self.child.stdout {
            Some(ref mut stdout) => {
                let mut buf = buf_pool.acquire();
                let write = buf.write_with_len(self.buf_len);
                match stdout.read(write) {
                    Ok(len) => {
                        write.truncate(len);
                        let buf = buf.share();
                        buf_pool.release(buf.clone());
                        Ok(Some(buf))
                    }
                    Err(_) => {
                        buf_pool.release(buf.share());
                        Err(())
                    }
                }
            }
            None => Ok(None),
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> bool {
        use io::Write;
        match self.child.stdin {
            Some(ref mut stdin) => stdin.write_all(buf).is_ok(),
            None => true,
        }
    }

    pub fn close_input(&mut self) {
        self.child.stdin = None;
    }

    pub fn kill(&mut self) {
        if !self.alive {
            return;
        }

        self.alive = false;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
        self.alive = false;
    }
}

pub fn errno() -> libc::c_int {
    unsafe { *libc::__errno_location() }
}

pub fn suspend_process(application: &mut ClientApplication, raw_mode: &mut Option<RawMode>) {
    application.restore_screen();
    let was_in_raw_mode = raw_mode.is_some();
    *raw_mode = None;

    unsafe { libc::raise(libc::SIGTSTP) };

    if was_in_raw_mode {
        *raw_mode = Some(RawMode::enter());
    }
    application.reinit_screen();
}

pub fn get_terminal_size() -> (usize, usize) {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::ioctl(
            libc::STDOUT_FILENO,
            libc::TIOCGWINSZ as _,
            &mut size as *mut libc::winsize,
        )
    };
    if result == -1 || size.ws_col == 0 {
        panic!("could not get terminal size");
    }

    (size.ws_col as _, size.ws_row as _)
}

pub fn parse_terminal_keys(mut buf: &[u8], backspace_code: u8, keys: &mut Vec<Key>) {
    loop {
        let (key, rest) = match buf {
            &[] => break,
            &[b, ref rest @ ..] if b == backspace_code => (Key::Backspace, rest),
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
            &[b'\r', ref rest @ ..] => (Key::Enter, rest),
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

