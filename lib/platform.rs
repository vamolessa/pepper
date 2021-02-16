use std::{io, process::Command, sync::Arc};

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

// TODO: rename to PlatformServerEvent
// TODO: add ProcessSpawned { handle: PlatformProcessHandle, tag: PlatformProcessTag }
pub enum ServerPlatformEvent {
    ConnectionOpen {
        handle: PlatformConnectionHandle,
    },
    ConnectionClose {
        handle: PlatformConnectionHandle,
    },
    ConnectionMessage {
        handle: PlatformConnectionHandle,
        buf: PlatformBuf,
    },
    ProcessStdout {
        index: usize,
        len: usize,
    },
    ProcessStderr {
        index: usize,
        len: usize,
    },
    ProcessExit {
        index: usize,
        success: bool,
    },
}

pub enum PlatformServerRequest {
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

pub struct RawPlatformServerChannel {
    pub data: *mut (),
    pub request: fn(*mut (), PlatformServerRequest),
}
pub struct PlatformServerChannel(RawPlatformServerChannel);
impl PlatformServerChannel {
    unsafe fn from_raw(raw: RawPlatformServerChannel) -> Self {
        Self(raw)
    }

    #[inline]
    pub fn request(&self, request: PlatformServerRequest) {
        (self.0.request)(self.0.data, request);
    }
}

pub struct RawPlatformClipboard {
    pub read: fn(text: &mut String) -> bool,
    pub write: fn(text: &str),
}
pub struct PlatformClipboard(RawPlatformClipboard);
impl PlatformClipboard {
    pub unsafe fn from_raw(raw: RawPlatformClipboard) -> Self {
        Self(raw)
    }

    #[inline]
    pub fn read(&self, text: &mut String) -> bool {
        (self.0.read)(text)
    }

    #[inline]
    pub fn write(&self, text: &str) {
        (self.0.write)(text);
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
    pub fn aquire(&mut self) -> PlatformBuf {
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

pub trait Platform {
    fn write_to_connection(&self, index: usize, buf: &[u8]) -> bool;
    fn close_connection(&self, index: usize);

    fn spawn_process(
        &mut self,
        command: Command,
        stdout_buf_len: usize,
        stderr_buf_len: usize,
    ) -> io::Result<usize>;
    fn write_to_process(&self, index: usize, buf: &[u8]) -> bool;
    fn kill_process(&self, index: usize);
}
