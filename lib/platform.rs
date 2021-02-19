use std::{
    process::Command,
    sync::{mpsc, Arc},
};

use crate::application::ProcessTag;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    None,
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    Delete,
    F(u8),
    Char(char),
    Ctrl(char),
    Alt(char),
    Esc,
}

#[derive(Clone, Copy)]
pub struct PlatformConnectionHandle(pub usize);

#[derive(Clone, Copy)]
pub struct PlatformProcessHandle(pub usize);

pub enum PlatformServerRequest {
    Exit,
    WriteToConnection {
        handle: PlatformConnectionHandle,
        buf: SharedPlatformBuf,
    },
    CloseConnection {
        handle: PlatformConnectionHandle,
    },
    SpawnProcess {
        tag: ProcessTag,
        command: Command,
        stdout_buf_len: usize,
        stderr_buf_len: usize,
    },
    WriteToProcess {
        handle: PlatformProcessHandle,
        buf: SharedPlatformBuf,
    },
    KillProcess {
        handle: PlatformProcessHandle,
    },
}

pub struct Platform {
    read_from_clipboard: fn(&mut String) -> bool,
    write_to_clipboard: fn(&str),
    flush_requests: fn(),
    request_sender: mpsc::Sender<PlatformServerRequest>,
    needs_flushing: bool,
    pub buf_pool: PlatformBufPool,
}
impl Platform {
    pub fn new(
        read_from_clipboard: fn(&mut String) -> bool,
        write_to_clipboard: fn(&str),
        flush_requests: fn(),
        request_sender: mpsc::Sender<PlatformServerRequest>,
    ) -> Self {
        Self {
            read_from_clipboard,
            write_to_clipboard,
            flush_requests,
            request_sender,
            needs_flushing: false,
            buf_pool: PlatformBufPool::default(),
        }
    }

    pub fn read_from_clipboard(&self, text: &mut String) -> bool {
        (self.read_from_clipboard)(text)
    }

    pub fn write_to_clipboard(&self, text: &str) {
        (self.write_to_clipboard)(text)
    }

    pub fn enqueue_request(&mut self, request: PlatformServerRequest) {
        self.needs_flushing = true;
        let _ = self.request_sender.send(request);
    }

    pub fn flush_requests(&mut self) {
        if self.needs_flushing {
            self.needs_flushing = false;
            (self.flush_requests)();
        }
    }
}

pub struct ExclusivePlatformBuf(Arc<Vec<u8>>);
impl ExclusivePlatformBuf {
    pub fn share(self) -> SharedPlatformBuf {
        SharedPlatformBuf(self.0)
    }

    pub fn write(&mut self) -> &mut Vec<u8> {
        Arc::get_mut(&mut self.0).unwrap()
    }
}

#[derive(Clone)]
pub struct SharedPlatformBuf(Arc<Vec<u8>>);
impl SharedPlatformBuf {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Default)]
pub struct PlatformBufPool {
    bufs: Vec<SharedPlatformBuf>,
}
impl PlatformBufPool {
    pub fn acquire(&mut self) -> ExclusivePlatformBuf {
        for (i, buf) in self.bufs.iter_mut().enumerate() {
            if Arc::get_mut(&mut buf.0).is_some() {
                let buf = self.bufs.swap_remove(i);
                return ExclusivePlatformBuf(buf.0);
            }
        }

        ExclusivePlatformBuf(Arc::new(Vec::new()))
    }

    pub fn release(&mut self, buf: SharedPlatformBuf) {
        self.bufs.push(buf);
    }
}
