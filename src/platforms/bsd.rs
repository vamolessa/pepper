use std::{
    env, fs, io,
    os::unix::{
        ffi::OsStrExt,
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    path::Path,
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

const MAX_CLIENT_COUNT: usize = 20;
const MAX_PROCESS_COUNT: usize = 42;

pub fn main() {
    println!("hello from bsd");
}
