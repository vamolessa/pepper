use std::{
    io,
    process::{Command, Stdio},
    sync::{mpsc, Arc},
};

use crate::{client::ClientHandle, command::parse_process_command, lsp};

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
    read_from_clipboard: fn(&mut String),
    write_to_clipboard: fn(&str),
    flush_requests: fn(),
    request_sender: mpsc::Sender<PlatformRequest>,
    needs_flushing: bool,
    pub buf_pool: BufPool,

    pub copy_command: String,
    pub paste_command: String,
}
impl Platform {
    pub fn new(
        read_from_clipboard: fn(&mut String),
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
            copy_command: String::new(),
            paste_command: String::new(),
        }
    }

    pub fn read_from_clipboard(&self, text: &mut String) {
        text.clear();
        if self.paste_command.is_empty() {
            (self.read_from_clipboard)(text);
        } else if let Ok(mut command) = parse_process_command(&self.paste_command, "") {
            command.stdin(Stdio::null());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::null());
            if let Ok(output) = command.output() {
                if let Ok(output) = String::from_utf8(output.stdout) {
                    text.clear();
                    text.push_str(&output);
                }
            }
        }
    }

    pub fn write_to_clipboard(&self, text: &str) {
        if self.copy_command.is_empty() {
            (self.write_to_clipboard)(text);
        } else if let Ok(mut command) = parse_process_command(&self.copy_command, "") {
            command.stdin(Stdio::piped());
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
            if let Ok(mut child) = command.spawn() {
                if let Some(mut stdin) = child.stdin.take() {
                    use io::Write;
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
            }
        }
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

