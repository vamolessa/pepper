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

use crate::{
    application::{ApplicationConfig, ClientApplication},
    editor_utils::hash_bytes,
    platform::{BufPool, Key, PooledBuf, ProcessTag},
    Args,
};

pub(crate) fn run(
    config: ApplicationConfig,
    server_fn: fn(ApplicationConfig, UnixListener),
    client_fn: fn(Args, UnixStream),
) {
    let mut session_path = String::new();
    session_path.push_str("/tmp/");
    session_path.push_str(env!("CARGO_PKG_NAME"));
    session_path.push('/');

    match config.args.session {
        Some(ref name) => session_path.push_str(name),
        None => {
            use io::Write;

            let current_dir = env::current_dir().expect("could not retrieve the current directory");
            let current_dir_bytes = current_dir.as_os_str().as_bytes();
            let current_directory_hash = hash_bytes(current_dir_bytes);

            let mut hash_buf = [0u8; 16];
            let mut cursor = io::Cursor::new(&mut hash_buf[..]);
            write!(&mut cursor, "{:x}", current_directory_hash).unwrap();
            let len = cursor.position() as usize;
            let name = std::str::from_utf8(&hash_buf[..len]).unwrap();
            session_path.push_str(name);
        }
    }

    if config.args.print_session {
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

    if config.args.server {
        server_fn(config, start_server(session_path));
        let _ = fs::remove_file(session_path);
    } else {
        match UnixStream::connect(session_path) {
            Ok(stream) => client_fn(config.args, stream),
            Err(_) => match unsafe { libc::fork() } {
                -1 => panic!("could not start server"),
                0 => {
                    unsafe { libc::daemon(false as _, false as _) };
                    server_fn(config, start_server(session_path));
                    let _ = fs::remove_file(session_path);
                }
                _ => loop {
                    match UnixStream::connect(session_path) {
                        Ok(stream) => {
                            client_fn(config.args, stream);
                            break;
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(100)),
                    }
                },
            },
        }
    }
}

pub(crate) fn is_pipped(fd: RawFd) -> bool {
    unsafe { libc::isatty(fd) != true as _ }
}

pub(crate) struct Terminal {
    fd: RawFd,
    original_state: libc::termios,
}
impl Terminal {
    pub fn new() -> Self {
        let flags = libc::O_RDWR | libc::O_CLOEXEC;
        let fd = unsafe { libc::open("/dev/tty\0".as_ptr() as _, flags) };
        if fd < 0 {
            panic!("could not open terminal");
        }

        let original_state = unsafe {
            let mut original_state = std::mem::zeroed();
            libc::tcgetattr(fd, &mut original_state);
            original_state
        };

        Self { fd, original_state }
    }

    pub fn to_client_output(&self) -> ClientOutput {
        ClientOutput(self.fd)
    }

    pub fn enter_raw_mode(&self) {
        let mut next_state = self.original_state.clone();
        next_state.c_iflag &= !(libc::IGNBRK
            | libc::BRKINT
            | libc::PARMRK
            | libc::ISTRIP
            | libc::INLCR
            | libc::IGNCR
            | libc::ICRNL
            | libc::IXON);
        next_state.c_oflag &= !libc::OPOST;
        next_state.c_cflag &= !(libc::CSIZE | libc::PARENB);
        next_state.c_cflag |= libc::CS8;
        next_state.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
        next_state.c_lflag |= libc::NOFLSH;
        next_state.c_cc[libc::VMIN] = 0;
        next_state.c_cc[libc::VTIME] = 0;
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &next_state) };
    }

    pub fn leave_raw_mode(&self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.original_state) };
    }

    pub fn get_size(&self) -> (u16, u16) {
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::ioctl(
                self.fd,
                libc::TIOCGWINSZ as _,
                &mut size as *mut libc::winsize,
            )
        };
        if result == -1 || size.ws_col == 0 || size.ws_row == 0 {
            panic!("could not get terminal size");
        }

        (size.ws_col as _, size.ws_row as _)
    }

    pub fn parse_keys(&self, mut buf: &[u8], keys: &mut Vec<Key>) {
        let backspace_code = self.original_state.c_cc[libc::VERASE];
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
}
impl AsRawFd for Terminal {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.leave_raw_mode()
    }
}

pub(crate) fn read(fd: RawFd, buf: &mut [u8]) -> Result<usize, ()> {
    let len = unsafe { libc::read(fd, buf.as_mut_ptr() as _, buf.len()) };
    if len >= 0 {
        Ok(len as _)
    } else {
        Err(())
    }
}

pub(crate) fn write_all_bytes(fd: RawFd, mut buf: &[u8]) -> bool {
    while !buf.is_empty() {
        let len = unsafe { libc::write(fd, buf.as_ptr() as _, buf.len()) };
        if len > 0 {
            buf = &buf[len as usize..];
        } else {
            return false;
        }
    }

    true
}

pub(crate) fn read_from_connection(
    connection: &mut UnixStream,
    buf_pool: &mut BufPool,
    len: usize,
) -> Result<PooledBuf, ()> {
    use io::Read;
    let mut buf = buf_pool.acquire();
    let write = buf.write_with_len(len);
    match connection.read(write) {
        Ok(0) | Err(_) => {
            buf_pool.release(buf);
            Err(())
        }
        Ok(len) => {
            write.truncate(len);
            Ok(buf)
        }
    }
}

pub(crate) struct Process {
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

    pub fn read(&mut self, buf_pool: &mut BufPool) -> Result<Option<PooledBuf>, ()> {
        use io::Read;
        match self.child.stdout {
            Some(ref mut stdout) => {
                let mut buf = buf_pool.acquire();
                let write = buf.write_with_len(self.buf_len);
                match stdout.read(write) {
                    Ok(0) | Err(_) => {
                        buf_pool.release(buf);
                        Err(())
                    }
                    Ok(len) => {
                        write.truncate(len);
                        Ok(Some(buf))
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

pub(crate) fn suspend_process<O>(
    application: &mut ClientApplication<O>,
    terminal: Option<&Terminal>,
) where
    O: io::Write,
{
    application.restore_screen();
    if let Some(terminal) = terminal {
        terminal.leave_raw_mode();
    }

    unsafe { libc::raise(libc::SIGTSTP) };

    if let Some(terminal) = terminal {
        terminal.enter_raw_mode();
    }
    application.reinit_screen();
}

pub struct ClientOutput(RawFd);
impl io::Write for ClientOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = unsafe { libc::write(self.0, buf.as_ptr() as _, buf.len()) };
        if len >= 0 {
            Ok(len as _)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

