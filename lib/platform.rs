use std::{process::Command, sync::Arc};

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

pub trait Platform: Send + Sync {
    fn read_from_clipboard(&self, text: &mut String) -> bool;
    fn write_to_clipboard(&self, text: &str);
    fn enqueue_request(&self, request: PlatformServerRequest);
    fn flush_requests(&self);
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
