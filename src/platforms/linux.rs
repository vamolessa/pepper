use std::{
    env, io,
    os::unix::{ffi::OsStrExt, io::IntoRawFd},
    process::{Child, ChildStdin},
    ptr::NonNull,
    sync::{
        atomic::{AtomicPtr, Ordering},
        mpsc,
    },
    time::Duration,
};

use libc::{
    c_int, c_void, epoll_create1, sigaction, sigemptyset, siginfo_t, SA_SIGINFO, SIGINT,
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

    let mut uds_path = String::new();
    // TODO: rest of usd path here
    uds_path.push_str(session_name);

    if args.print_session {
        print!("{}", uds_path);
        return;
    }

    set_ctrlc_handler();

    for i in 0..5 {
        println!("contando {}", i);
        std::thread::sleep(Duration::from_secs(1))
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
