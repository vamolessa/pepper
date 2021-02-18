use std::{
    process::Command,
    sync::{mpsc, Arc},
};

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

pub enum PlatformProcessTag {
    Command(usize),
}

#[derive(Clone, Copy)]
pub struct PlatformConnectionHandle(pub usize);

#[derive(Clone, Copy)]
pub struct PlatformProcessHandle(pub usize);

pub enum PlatformServerRequest {
    Exit,
    WriteToConnection {
        handle: PlatformConnectionHandle,
        buf: PlatformBuf,
    },
    CloseConnection {
        handle: PlatformConnectionHandle,
    },
    SpawnProcess {
        tag: PlatformProcessTag,
        command: Command,
        stdout_buf_len: usize,
        stderr_buf_len: usize,
    },
    WriteToProcess {
        handle: PlatformProcessHandle,
        buf: PlatformBuf,
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

#[derive(Clone)]
pub struct PlatformBuf(Arc<Vec<u8>>);
impl PlatformBuf {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn write(&mut self) -> Option<&mut Vec<u8>> {
        Arc::get_mut(&mut self.0)
    }
}

#[derive(Default)]
pub struct PlatformBufPool {
    bufs: Vec<PlatformBuf>,
}
impl PlatformBufPool {
    pub fn acquire(&mut self) -> PlatformBuf {
        for (i, buf) in self.bufs.iter_mut().enumerate() {
            if Arc::get_mut(&mut buf.0).is_some() {
                return self.bufs.swap_remove(i);
            }
        }

        PlatformBuf(Arc::new(Vec::new()))
    }

    pub fn release(&mut self, buf: PlatformBuf) {
        self.bufs.push(buf);
    }
}
