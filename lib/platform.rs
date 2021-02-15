use std::{io, process::Command};

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
pub enum ServerPlatformEvent {
    ConnectionOpen { index: usize },
    ConnectionClose { index: usize },
    ConnectionMessage { index: usize, len: usize },
    ProcessStdout { index: usize, len: usize },
    ProcessStderr { index: usize, len: usize },
    ProcessExit { index: usize, success: bool },
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
        (self.0.write)(text)
    }
}

pub trait Platform {
    fn read_from_connection(&self, index: usize, len: usize) -> &[u8];
    fn write_to_connection(&self, index: usize, buf: &[u8]) -> bool;
    fn close_connection(&self, index: usize);

    fn spawn_process(
        &mut self,
        command: Command,
        stdout_buf_len: usize,
        stderr_buf_len: usize,
    ) -> io::Result<usize>;
    fn read_from_process_stdout(&self, index: usize, len: usize) -> &[u8];
    fn read_from_process_stderr(&self, index: usize, len: usize) -> &[u8];
    fn write_to_process(&self, index: usize, buf: &[u8]) -> bool;
    fn kill_process(&self, index: usize);
}
