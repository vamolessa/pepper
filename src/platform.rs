use std::{
    io,
    process::{Command, Stdio},
    sync::{mpsc, Arc},
};

use crate::{client::ClientHandle, editor_utils::parse_process_command, lsp};

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
    Quit,
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
    Lsp(lsp::ClientHandle),
}

#[derive(Clone, Copy)]
pub struct ProcessHandle(pub usize);

pub struct Platform {
    read_from_clipboard: Option<fn(&mut String)>,
    write_to_clipboard: Option<fn(&str)>,
    flush_requests: fn(),
    request_sender: mpsc::Sender<PlatformRequest>,
    needs_flushing: bool,
    pub buf_pool: BufPool,

    internal_clipboard: String,
    pub copy_command: String,
    pub paste_command: String,
}
impl Platform {
    pub fn new(flush_requests: fn(), request_sender: mpsc::Sender<PlatformRequest>) -> Self {
        Self {
            read_from_clipboard: None,
            write_to_clipboard: None,
            flush_requests,
            request_sender,
            needs_flushing: false,
            buf_pool: BufPool::default(),
            internal_clipboard: String::new(),
            copy_command: String::new(),
            paste_command: String::new(),
        }
    }

    pub fn set_clipboard_api(
        &mut self,
        read_from_clipboard: fn(&mut String),
        write_to_clipboard: fn(&str),
    ) {
        self.read_from_clipboard = Some(read_from_clipboard);
        self.write_to_clipboard = Some(write_to_clipboard);
    }

    pub fn read_from_clipboard(&self, text: &mut String) {
        if let Some(mut command) = parse_process_command(&self.paste_command) {
            command.stdin(Stdio::null());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::null());
            if let Ok(output) = command.output() {
                if let Ok(output) = String::from_utf8(output.stdout) {
                    text.clear();
                    text.push_str(&output);
                }
            }
        } else if let Some(read_from_clipboard) = self.read_from_clipboard {
            read_from_clipboard(text);
        } else {
            text.push_str(&self.internal_clipboard);
        }
    }

    pub fn write_to_clipboard(&mut self, text: &str) {
        if let Some(mut command) = parse_process_command(&self.copy_command) {
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
        } else if let Some(write_to_clipboard) = self.write_to_clipboard {
            write_to_clipboard(text);
        } else {
            self.internal_clipboard.clear();
            self.internal_clipboard.push_str(text);
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

// TODO: later try to make a SharedPool<T>
// which is globally available and lock free
// maybe even an arena/bump/temp allocator
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
