use std::{
    process::Command,
    sync::{mpsc, Arc},
};

use crate::{client::ClientHandle, lsp};

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

pub enum PlatformRequest {
    Exit,
    WriteToClient {
        handle: ClientHandle,
        buf: SharedBuf,
    },
    CloseClient {
        handle: ClientHandle,
    },
    SpawnProcess {
        tag: ProcessTag,
        command: Command,
        buf_len: usize,
    },
    WriteToProcess {
        handle: ProcessHandle,
        buf: SharedBuf,
    },
    CloseProcessInput {
        handle: ProcessHandle,
    },
    KillProcess {
        handle: ProcessHandle,
    },
}

#[derive(Clone, Copy)]
pub enum ProcessTag {
    Buffer(usize),
    Command(usize),
    Lsp(lsp::ClientHandle),
}

#[derive(Clone, Copy)]
pub struct ProcessHandle(pub usize);

pub struct Platform {
    read_from_clipboard: fn(&mut String) -> bool,
    write_to_clipboard: fn(&str),
    flush_requests: fn(),
    request_sender: mpsc::Sender<PlatformRequest>,
    needs_flushing: bool,
    pub buf_pool: BufPool,
}
impl Platform {
    pub fn new(
        read_from_clipboard: fn(&mut String) -> bool,
        write_to_clipboard: fn(&str),
        flush_requests: fn(),
        request_sender: mpsc::Sender<PlatformRequest>,
    ) -> Self {
        Self {
            read_from_clipboard,
            write_to_clipboard,
            flush_requests,
            request_sender,
            needs_flushing: false,
            buf_pool: BufPool::default(),
        }
    }

    pub fn read_from_clipboard(&self, text: &mut String) -> bool {
        (self.read_from_clipboard)(text)
    }

    pub fn write_to_clipboard(&self, text: &str) {
        (self.write_to_clipboard)(text)
    }

    pub fn enqueue_request(&mut self, request: PlatformRequest) {
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

pub struct ExclusiveBuf(Arc<Vec<u8>>);
impl ExclusiveBuf {
    pub fn share(self) -> SharedBuf {
        SharedBuf(self.0)
    }

    pub fn write(&mut self) -> &mut Vec<u8> {
        let buf = Arc::get_mut(&mut self.0).unwrap();
        buf.clear();
        buf
    }

    pub fn write_with_len(&mut self, len: usize) -> &mut Vec<u8> {
        let buf = Arc::get_mut(&mut self.0).unwrap();
        buf.resize(len, 0);
        buf
    }
}

#[derive(Clone)]
pub struct SharedBuf(Arc<Vec<u8>>);
impl SharedBuf {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Default)]
pub struct BufPool {
    pool: Vec<SharedBuf>,
}
impl BufPool {
    pub fn acquire(&mut self) -> ExclusiveBuf {
        for (i, buf) in self.pool.iter_mut().enumerate() {
            if Arc::get_mut(&mut buf.0).is_some() {
                let buf = self.pool.swap_remove(i);
                return ExclusiveBuf(buf.0);
            }
        }

        ExclusiveBuf(Arc::new(Vec::new()))
    }

    pub fn release(&mut self, buf: SharedBuf) {
        self.pool.push(buf);
    }
}