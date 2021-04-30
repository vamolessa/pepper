use std::{
    env, fs, io,
    os::unix::{
        ffi::OsStrExt,
        io::IntoRawFd,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::{Child, ChildStdin},
    sync::mpsc,
    time::Duration,
};

use libc::{
    c_int, c_void, epoll_create1, fork, sigaction, sigemptyset, siginfo_t, SA_SIGINFO, SIGINT,
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
    stream_path.push_str("/tmp/pepper/");
    stream_path.push_str(session_name);

    if args.print_session {
        print!("{}", stream_path);
        return;
    }

    set_ctrlc_handler();

    let stream_path = Path::new(&stream_path);

    if args.force_server {
        run_server(stream_path);
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
            _ => run_server(stream_path),
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

fn run_server(stream_path: &Path) {
    if let Some(dir) = stream_path.parent() {
        if !dir.exists() {
            let _ = fs::create_dir(dir);
        }
    }

    let listener = match UnixListener::bind(stream_path) {
        Ok(listener) => listener,
        Err(_) => {
            let _ = fs::remove_file(stream_path);
            UnixListener::bind(stream_path).expect("could not start unix domain socket server")
        }
    };

    println!("begin server");
    std::thread::sleep(Duration::from_secs(3));
    println!("end server");
    // TODO
    
    fs::remove_file(stream_path);
}

fn run_client(args: Args, stream: UnixStream) {
    // TODO
}
