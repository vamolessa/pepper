use std::{
    env, fs, io,
    process::Child,
    os::unix::{
        ffi::OsStrExt,
        io::{AsRawFd, RawFd},
        net::UnixStream,
    },
    path::Path,
    time::Duration,
};

use pepper::{
    application::AnyError,
    editor_utils::hash_bytes,
    platform::{BufPool, ProcessTag, SharedBuf},
    Args,
};

pub fn run(
    server_fn: fn(stream_path: &Path) -> Result<(), AnyError>,
    client_fn: fn(args: Args, connection: UnixStream),
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

    if args.as_server {
        let _ = server_fn(session_path);
        let _ = fs::remove_file(session_path);
    } else {
        match UnixStream::connect(session_path) {
            Ok(stream) => client_fn(args, stream),
            Err(_) => match unsafe { libc::fork() } {
                -1 => panic!("could not start server"),
                0 => loop {
                    match UnixStream::connect(session_path) {
                        Ok(stream) => {
                            client_fn(args, stream);
                            break;
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(100)),
                    }
                },
                _ => {
                    let _ = server_fn(session_path);
                    let _ = fs::remove_file(session_path);
                }
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
            new.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
            new.c_oflag &= !libc::OPOST;
            new.c_cflag |= libc::CS8;
            new.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
            new.c_cc[libc::VMIN] = 0;
            new.c_cc[libc::VTIME] = 1;
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &new);
            original
        };
        Self { original }
    }
}
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.original) };
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
                        Ok(Some(buf.share()))
                    }
                    Err(_) => Err(()),
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

