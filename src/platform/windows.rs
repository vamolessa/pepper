use std::{
    convert::Into,
    io,
    ops::{Deref, DerefMut},
    os::windows::io::AsRawHandle,
    process::{Child, Command, Stdio},
    time::Duration,
};

use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, FALSE, TRUE},
        ntdef::NULL,
        winerror::{ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_CONNECTED, WAIT_TIMEOUT},
    },
    um::{
        consoleapi::{
            AllocConsole, GetConsoleMode, ReadConsoleInputW, SetConsoleCtrlHandler, SetConsoleMode,
        },
        errhandlingapi::GetLastError,
        fileapi::{CreateFileW, FindFirstFileW, ReadFile, WriteFile, OPEN_EXISTING},
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        ioapiset::GetOverlappedResult,
        minwinbase::OVERLAPPED,
        namedpipeapi::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, SetNamedPipeHandleState,
        },
        processenv::{GetCommandLineW, GetStdHandle},
        processthreadsapi::{CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW},
        synchapi::{CreateEventW, SetEvent, WaitForMultipleObjects},
        winbase::{
            FILE_FLAG_OVERLAPPED, INFINITE, NORMAL_PRIORITY_CLASS, PIPE_ACCESS_DUPLEX,
            PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, STARTF_USESTDHANDLES,
            STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, WAIT_ABANDONED_0, WAIT_OBJECT_0,
        },
        wincon::{
            FreeConsole, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            ENABLE_WINDOW_INPUT,
        },
        wincontypes::{
            INPUT_RECORD, KEY_EVENT, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED,
            RIGHT_CTRL_PRESSED, SHIFT_PRESSED, WINDOW_BUFFER_SIZE_EVENT,
        },
        winnt::{GENERIC_READ, GENERIC_WRITE, HANDLE, MAXIMUM_WAIT_OBJECTS},
        winuser::{
            VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_LEFT,
            VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
        },
    },
};

use crate::platform::{
    ClientApplication, ClientEvent, ConnectionHandle, Key, Platform, ProcessHandle,
    ServerApplication, ServerEvent, WriteResult,
};

const SERVER_PIPE_BUFFER_LEN: usize = 512;
const CLIENT_PIPE_BUFFER_LEN: usize = 2 * 1024;
const CHILD_BUFFER_LEN: usize = 2 * 1024;

pub fn run<S, C>()
where
    S: ServerApplication,
    C: ClientApplication,
{
    unsafe extern "system" fn ctrl_handler(_ctrl_type: DWORD) -> BOOL {
        FALSE
    }

    if unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) } == FALSE {
        panic!("could not set ctrl handler");
    }

    let session_name = "pepper_session_name";
    let mut pipe_path = Vec::new();
    pipe_path.extend("\\\\.\\pipe\\".encode_utf16());
    pipe_path.extend(session_name.encode_utf16());
    pipe_path.push(0);

    let input_handle = get_std_handle(STD_INPUT_HANDLE);
    let output_handle = get_std_handle(STD_OUTPUT_HANDLE);

    match (input_handle, output_handle) {
        (Some(input_handle), Some(output_handle)) => {
            println!("run client");
            unsafe { run_client(&pipe_path, input_handle, output_handle) };
        }
        _ => {
            println!("run server");
            unsafe { run_server::<S>(&pipe_path) };
        }
    }
}

fn get_last_error() -> DWORD {
    unsafe { GetLastError() }
}

fn file_exists(path: &[u16]) -> bool {
    unsafe { FindFirstFileW(path.as_ptr(), &mut Default::default()) != INVALID_HANDLE_VALUE }
}

fn get_std_handle(which: DWORD) -> Option<HANDLE> {
    let handle = unsafe { GetStdHandle(which) };
    if handle != NULL && handle != INVALID_HANDLE_VALUE {
        Some(handle)
    } else {
        None
    }
}

fn fork() {
    let mut startup_info = STARTUPINFOW::default();
    startup_info.cb = std::mem::size_of::<STARTUPINFOW>() as _;
    startup_info.dwFlags = STARTF_USESTDHANDLES;
    startup_info.hStdInput = INVALID_HANDLE_VALUE;
    startup_info.hStdOutput = INVALID_HANDLE_VALUE;
    startup_info.hStdError = INVALID_HANDLE_VALUE;

    let mut process_info = PROCESS_INFORMATION::default();

    let result = unsafe {
        CreateProcessW(
            std::ptr::null(),
            GetCommandLineW(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            FALSE,
            NORMAL_PRIORITY_CLASS,
            NULL,
            std::ptr::null_mut(),
            &mut startup_info,
            &mut process_info,
        )
    };

    let _ = Handle(process_info.hProcess);
    let _ = Handle(process_info.hThread);

    if result == FALSE {
        panic!("could not spawn server");
    }
}

enum WaitResult {
    Signaled(usize),
    Abandoned(usize),
    Timeout,
}
fn wait_for_multiple_objects(handles: &[HANDLE], timeout: Option<Duration>) -> WaitResult {
    let timeout = match timeout {
        Some(duration) => duration.as_millis() as _,
        None => INFINITE,
    };
    let len = MAXIMUM_WAIT_OBJECTS.min(handles.len() as DWORD);
    let result = unsafe { WaitForMultipleObjects(len, handles.as_ptr(), FALSE, timeout) };
    if result == WAIT_TIMEOUT {
        WaitResult::Timeout
    } else if result >= WAIT_OBJECT_0 && result < (WAIT_OBJECT_0 + len) {
        WaitResult::Signaled((result - WAIT_OBJECT_0) as _)
    } else if result >= WAIT_ABANDONED_0 && result < (WAIT_ABANDONED_0 + len) {
        WaitResult::Abandoned((result - WAIT_ABANDONED_0) as _)
    } else {
        panic!("could not wait for event")
    }
}

fn make_buffer(len: usize) -> Box<[u8]> {
    let mut buf = Vec::with_capacity(len);
    buf.resize(len, 0);
    buf.into_boxed_slice()
}

struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Event(HANDLE);
impl Event {
    pub fn new() -> Self {
        let handle = unsafe { CreateEventW(std::ptr::null_mut(), TRUE, TRUE, std::ptr::null()) };
        if handle == NULL {
            panic!("could not create event");
        }
        Self(handle)
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    pub fn notify(&self) {
        if unsafe { SetEvent(self.0) } == FALSE {
            panic!("could not set event");
        }
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Overlapped(OVERLAPPED);
impl Overlapped {
    pub fn with_event(event: &Event) -> Self {
        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = event.handle();
        Self(overlapped)
    }

    pub fn as_mut_ptr(&mut self) -> *mut OVERLAPPED {
        &mut self.0
    }
}

enum ReadResult {
    Waiting,
    Ok(usize),
    Err,
}

struct AsyncIO {
    handle: Handle,
    overlapped: Overlapped,
    event: Event,
    pending_io: bool,
    buf: Box<[u8]>,
    read_len: usize,
}
impl AsyncIO {
    pub fn from_handle(handle: Handle, buf_len: usize) -> Self {
        let event = Event::new();
        let overlapped = Overlapped::with_event(&event);

        let mut buf = Vec::with_capacity(buf_len);
        buf.resize(buf_len, 0);
        let buf = buf.into_boxed_slice();

        Self {
            handle,
            overlapped,
            event,
            pending_io: false,
            buf,
            read_len: 0,
        }
    }

    pub fn read_async(&mut self) -> ReadResult {
        self.read_len = 0;
        let mut read_len = 0;
        if self.pending_io {
            let result = unsafe {
                GetOverlappedResult(
                    self.handle.0,
                    self.overlapped.as_mut_ptr(),
                    &mut read_len,
                    FALSE,
                )
            };

            if result == FALSE {
                match get_last_error() {
                    ERROR_MORE_DATA => {
                        self.pending_io = false;
                        self.event.notify();
                        self.read_len = read_len as _;
                        ReadResult::Ok(self.read_len)
                    }
                    _ => {
                        self.pending_io = false;
                        ReadResult::Err
                    }
                }
            } else {
                self.pending_io = false;
                self.event.notify();
                self.read_len = read_len as _;
                ReadResult::Ok(self.read_len)
            }
        } else {
            let result = unsafe {
                ReadFile(
                    self.handle.0,
                    self.buf.as_mut_ptr() as _,
                    self.buf.len() as _,
                    &mut read_len,
                    self.overlapped.as_mut_ptr(),
                )
            };

            if result == FALSE {
                match get_last_error() {
                    ERROR_IO_PENDING => {
                        self.pending_io = true;
                        ReadResult::Waiting
                    }
                    _ => {
                        self.pending_io = false;
                        ReadResult::Err
                    }
                }
            } else {
                self.pending_io = false;
                self.event.notify();
                self.read_len = read_len as _;
                ReadResult::Ok(self.read_len)
            }
        }
    }

    pub fn get_read_bytes(&self) -> &[u8] {
        &self.buf[..self.read_len]
    }

    pub fn write(&mut self, buf: &[u8]) -> WriteResult {
        // TODO: write all bytes!
        let mut write_len = 0;
        let result = unsafe {
            WriteFile(
                self.handle.0,
                buf.as_ptr() as _,
                buf.len() as _,
                &mut write_len,
                std::ptr::null_mut(),
            )
        };

        if result == FALSE {
            WriteResult::Err
        } else {
            WriteResult::Ok
        }
    }
}

struct PipeToClient(AsyncIO);
impl Deref for PipeToClient {
    type Target = AsyncIO;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for PipeToClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for PipeToClient {
    fn drop(&mut self) {
        unsafe {
            DisconnectNamedPipe(self.0.handle.0);
        }
    }
}

struct PipeToServer(AsyncIO);
impl PipeToServer {
    pub fn connect(path: &[u16]) -> Self {
        let pipe_handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                NULL,
            )
        };
        if pipe_handle == INVALID_HANDLE_VALUE {
            panic!("could not establish a connection {}", get_last_error());
        }

        let mut mode = PIPE_READMODE_BYTE;
        let result = unsafe {
            SetNamedPipeHandleState(
                pipe_handle,
                &mut mode,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };

        if result == FALSE {
            panic!("could not establish a connection");
        }

        let pipe_handle = Handle(pipe_handle);
        let io = AsyncIO::from_handle(pipe_handle, CLIENT_PIPE_BUFFER_LEN);
        io.event.notify();
        Self(io)
    }
}
impl Deref for PipeToServer {
    type Target = AsyncIO;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for PipeToServer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

struct PipeToClientListener {
    io: AsyncIO,
}
impl PipeToClientListener {
    pub fn new(pipe_path: &[u16]) -> Self {
        let pipe_handle = unsafe {
            CreateNamedPipeW(
                pipe_path.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE,
                PIPE_UNLIMITED_INSTANCES,
                SERVER_PIPE_BUFFER_LEN as _,
                SERVER_PIPE_BUFFER_LEN as _,
                0,
                std::ptr::null_mut(),
            )
        };
        if pipe_handle == INVALID_HANDLE_VALUE {
            panic!("could not create new connection");
        }

        let pipe_handle = Handle(pipe_handle);
        let mut pipe = AsyncIO::from_handle(pipe_handle, SERVER_PIPE_BUFFER_LEN);

        if unsafe { ConnectNamedPipe(pipe.handle.0, pipe.overlapped.as_mut_ptr()) } != FALSE {
            panic!("could not accept incomming connection");
        }

        pipe.pending_io = match get_last_error() {
            ERROR_IO_PENDING => true,
            ERROR_PIPE_CONNECTED => {
                pipe.event.notify();
                false
            }
            _ => panic!("could not accept incomming connection"),
        };

        pipe.overlapped = Overlapped::with_event(&pipe.event);
        Self { io: pipe }
    }

    pub fn accept(&mut self, pipe_path: &[u16]) -> Option<PipeToClient> {
        match self.io.read_async() {
            ReadResult::Waiting => None,
            ReadResult::Ok(_) => {
                let mut io = Self::new(pipe_path).io;
                std::mem::swap(&mut self.io, &mut io);
                Some(PipeToClient(io))
            }
            ReadResult::Err => panic!("could not accept connection {}", get_last_error()),
        }
    }
}

enum ChildPipe {
    Open(AsyncIO),
    Closed,
}
impl ChildPipe {
    pub fn from_handle(handle: Option<Handle>) -> Self {
        match handle {
            Some(h) => {
                let io = AsyncIO::from_handle(h, CHILD_BUFFER_LEN);
                io.event.notify();
                Self::Open(io)
            }
            None => Self::Closed,
        }
    }
}
impl Drop for ChildPipe {
    fn drop(&mut self) {
        let mut pipe = Self::Closed;
        std::mem::swap(self, &mut pipe);
        std::mem::forget(pipe);
    }
}

struct AsyncChild {
    child: Child,
    stdout: ChildPipe,
    stderr: ChildPipe,
}
impl AsyncChild {
    pub fn from_child(child: Child) -> Self {
        let stdout = ChildPipe::from_handle(
            child
                .stdout
                .as_ref()
                .map(AsRawHandle::as_raw_handle)
                .map(Handle),
        );
        let stderr = ChildPipe::from_handle(
            child
                .stderr
                .as_ref()
                .map(AsRawHandle::as_raw_handle)
                .map(Handle),
        );

        Self {
            child,
            stdout,
            stderr,
        }
    }
}

enum EventSource {
    ConnectionListener,
    Connection(usize),
    ChildStdout(usize),
    ChildStderr(usize),
}
#[derive(Default)]
struct Events {
    wait_handles: Vec<HANDLE>,
    sources: Vec<EventSource>,
}
impl Events {
    pub fn track(&mut self, event: &Event, source: EventSource) {
        self.wait_handles.push(event.handle());
        self.sources.push(source);
    }

    pub fn wait_one(&mut self, timeout: Option<Duration>) -> Option<EventSource> {
        let result = match wait_for_multiple_objects(&self.wait_handles, timeout) {
            WaitResult::Signaled(i) => Some(self.sources.swap_remove(i)),
            WaitResult::Abandoned(_) => unreachable!(),
            WaitResult::Timeout => None,
        };

        self.wait_handles.clear();
        self.sources.clear();
        result
    }
}

struct ReadBuf(Box<[u8]>);
impl ReadBuf {
    pub fn with_len(len: usize) -> Self {
        let mut buf = Vec::with_capacity(len);
        buf.resize(len, 0);
        Self(buf.into_boxed_slice())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn slice(&self, len: usize) -> &[u8] {
        &self.0[..len]
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }
}

struct SlotVec<T>(Vec<Option<T>>);
impl<T> SlotVec<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, item: T) -> usize {
        let len = self.0.len();
        for i in 0..len {
            if let None = &self.0[i] {
                self.0[i] = Some(item);
                return i;
            }
        }

        self.0.push(Some(item));
        len
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.0.len() {
            let entry = &mut self.0[index];
            if entry.is_some() {
                *entry = None;
                if let Some(i) = self.0.iter().rposition(Option::is_some) {
                    self.0.truncate(i + 1);
                } else {
                    self.0.clear();
                }
            }
        }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.0[index].as_ref()
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.0[index].as_mut()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter<'a>(&'a self) -> impl 'a + Iterator<Item = (usize, &'a T)> {
        self.0.iter().enumerate().filter_map(|(i, e)| match e {
            Some(e) => Some((i, e)),
            None => None,
        })
    }
}

struct State {
    events: Events,
    listener: PipeToClientListener,
    pipes: SlotVec<PipeToClient>,
    children: SlotVec<AsyncChild>,
}

impl Platform for State {
    fn read_from_connection(&self, handle: ConnectionHandle) -> &[u8] {
        match self.pipes.get(handle.0) {
            Some(pipe) => pipe.get_read_bytes(),
            None => &[],
        }
    }

    fn write_to_connection(&mut self, handle: ConnectionHandle, buf: &[u8]) -> WriteResult {
        match self.pipes.get_mut(handle.0) {
            Some(pipe) => match pipe.write(buf) {
                WriteResult::Ok => WriteResult::Ok,
                WriteResult::Err => {
                    self.pipes.remove(handle.0);
                    WriteResult::Err
                }
            },
            None => WriteResult::Ok,
        }
    }

    fn close_connection(&mut self, handle: ConnectionHandle) {
        self.pipes.remove(handle.0);
    }

    fn spawn_process(&mut self, mut command: Command) -> io::Result<ProcessHandle> {
        let child = command.spawn()?;
        let index = self.children.push(AsyncChild::from_child(child));
        Ok(ProcessHandle(index))
    }

    fn read_from_process_stdout(&self, handle: ProcessHandle) -> &[u8] {
        match self.children.get(handle.0) {
            Some(child) => match child.stdout {
                ChildPipe::Open(ref io) => io.get_read_bytes(),
                ChildPipe::Closed => &[],
            },
            None => &[],
        }
    }

    fn read_from_process_stderr(&self, handle: ProcessHandle) -> &[u8] {
        match self.children.get(handle.0) {
            Some(child) => match child.stderr {
                ChildPipe::Open(ref io) => io.get_read_bytes(),
                ChildPipe::Closed => &[],
            },
            None => &[],
        }
    }

    fn write_to_process(&mut self, handle: ProcessHandle, buf: &[u8]) -> WriteResult {
        if let Some(child) = self.children.get_mut(handle.0) {
            if let Some(ref mut stdin) = child.child.stdin {
                use io::Write;
                if let Err(_) = stdin.write_all(buf) {
                    let _ = child.child.kill();
                    let _ = child.child.wait();
                    return WriteResult::Err;
                }
            }
        }

        WriteResult::Ok
    }

    fn kill_process(&mut self, handle: ProcessHandle) {
        if let Some(child) = self.children.get_mut(handle.0) {
            let _ = child.child.kill();
            let _ = child.child.wait();
            self.children.remove(handle.0);
        }
    }
}

unsafe fn run_server<A>(pipe_path: &[u16])
where
    A: ServerApplication,
{
    if file_exists(pipe_path) {
        return;
    }

    let mut state = State {
        events: Events::default(),
        listener: PipeToClientListener::new(pipe_path),
        pipes: SlotVec::new(),
        children: SlotVec::new(),
    };

    let mut application = match A::new(&mut state) {
        Some(application) => application,
        None => return,
    };

    macro_rules! send_event {
        ($event:expr) => {
            if !application.on_event(&mut state, $event) {
                break;
            }
        };
    }

    loop {
        state
            .events
            .track(&state.listener.io.event, EventSource::ConnectionListener);
        for (i, pipe) in state.pipes.iter() {
            state.events.track(&pipe.event, EventSource::Connection(i));
        }
        for (i, child) in state.children.iter() {
            if let ChildPipe::Open(io) = &child.stdout {
                state.events.track(&io.event, EventSource::ChildStdout(i));
            }
            if let ChildPipe::Open(io) = &child.stderr {
                state.events.track(&io.event, EventSource::ChildStderr(i));
            }
        }

        match state.events.wait_one(None) {
            Some(EventSource::ConnectionListener) => {
                if let Some(pipe) = state.listener.accept(pipe_path) {
                    let index = state.pipes.push(pipe);
                    let handle = ConnectionHandle(index);
                    send_event!(ServerEvent::ConnectionOpen(handle));
                }
            }
            Some(EventSource::Connection(i)) => {
                let pipe = match state.pipes.get_mut(i) {
                    Some(pipe) => pipe,
                    None => continue,
                };

                let handle = ConnectionHandle(i);
                match pipe.read_async() {
                    ReadResult::Waiting => (),
                    ReadResult::Err | ReadResult::Ok(0) => {
                        state.pipes.remove(i);
                        send_event!(ServerEvent::ConnectionClose(handle));
                    }
                    ReadResult::Ok(_) => {
                        let bytes = pipe.get_read_bytes();
                        match Key::parse(&mut bytes.iter().map(|b| *b as _)) {
                            Ok(Key::Ctrl('r')) => {
                                println!("execute program");
                                let child = std::process::Command::new("fzf")
                                    .stdin(std::process::Stdio::null())
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::null())
                                    .spawn()
                                    .unwrap();
                                state.children.push(AsyncChild::from_child(child));
                            }
                            _ => (),
                        }

                        let message = String::from_utf8_lossy(bytes);
                        println!(
                            "received {} bytes from client {}! message: '{}'",
                            bytes.len(),
                            i,
                            message
                        );

                        let message = b"thank you for your message!";
                        match pipe.write(message) {
                            WriteResult::Ok => (),
                            WriteResult::Err => {
                                state.pipes.remove(i);
                                if state.pipes.is_empty() {
                                    break;
                                }
                            }
                        }

                        send_event!(ServerEvent::ConnectionMessage(handle));
                    }
                }
            }
            Some(EventSource::ChildStdout(i)) => {
                let child = match state.children.get_mut(i) {
                    Some(child) => child,
                    None => continue,
                };
                let io = match child.stdout {
                    ChildPipe::Open(ref mut io) => io,
                    ChildPipe::Closed => continue,
                };

                let handle = ProcessHandle(i);
                match io.read_async() {
                    ReadResult::Waiting => (),
                    ReadResult::Err | ReadResult::Ok(0) => {
                        child.stdout = ChildPipe::Closed;
                        if let ChildPipe::Closed = child.stderr {
                            let status = child.child.wait().unwrap();
                            state.children.remove(i);
                            send_event!(ServerEvent::ProcessExit(handle, status));
                        }
                    }
                    ReadResult::Ok(_) => {
                        let bytes = io.get_read_bytes();
                        let message = String::from_utf8_lossy(bytes);
                        println!(
                            "received {} bytes from child {}! message: '{}'",
                            bytes.len(),
                            i,
                            message
                        );

                        send_event!(ServerEvent::ProcessStdout(handle));
                    }
                }
            }
            Some(EventSource::ChildStderr(i)) => {
                let child = match state.children.get_mut(i) {
                    Some(child) => child,
                    None => continue,
                };
                let io = match child.stderr {
                    ChildPipe::Open(ref mut io) => io,
                    ChildPipe::Closed => continue,
                };

                let handle = ProcessHandle(i);
                match io.read_async() {
                    ReadResult::Waiting => (),
                    ReadResult::Err | ReadResult::Ok(0) => {
                        child.stderr = ChildPipe::Closed;
                        if let ChildPipe::Closed = child.stdout {
                            let status = child.child.wait().unwrap();
                            state.children.remove(i);
                            send_event!(ServerEvent::ProcessExit(handle, status));
                        }
                    }
                    ReadResult::Ok(_) => {
                        let bytes = io.get_read_bytes();
                        let message = String::from_utf8_lossy(bytes);
                        println!(
                            "received {} bytes from child {}! message: '{}'",
                            bytes.len(),
                            i,
                            message
                        );

                        send_event!(ServerEvent::ProcessStderr(handle));
                    }
                }
            }
            None => println!("timeout waiting"),
        }
    }

    println!("finish server");
}

unsafe fn run_client(pipe_path: &[u16], input_handle: HANDLE, output_handle: HANDLE) {
    if !file_exists(pipe_path) {
        println!("pipe does not exist. running server...");

        fork();

        while !file_exists(pipe_path) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    let mut pipe = PipeToServer::connect(pipe_path);

    let mut original_input_mode = DWORD::default();
    if GetConsoleMode(input_handle, &mut original_input_mode) == FALSE {
        panic!("could not retrieve original console input mode");
    }
    if SetConsoleMode(input_handle, ENABLE_WINDOW_INPUT) == FALSE {
        panic!("could not set console input mode");
    }

    let mut original_output_mode = DWORD::default();
    if GetConsoleMode(output_handle, &mut original_output_mode) == FALSE {
        panic!("could not retrieve original console output mode");
    }
    if SetConsoleMode(
        output_handle,
        ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    ) == FALSE
    {
        panic!("could not set console output mode");
    }

    match pipe.write(b"hello there!") {
        WriteResult::Ok => (),
        WriteResult::Err => panic!("could not send message to server"),
    }

    let event_buffer = &mut [INPUT_RECORD::default(); 32][..];
    let wait_handles = [input_handle, pipe.event.handle()];

    'main_loop: loop {
        let wait_handle_index = match wait_for_multiple_objects(&wait_handles, None) {
            WaitResult::Signaled(i) => i,
            _ => continue,
        };
        match wait_handle_index {
            0 => {
                let mut event_count: DWORD = 0;
                if ReadConsoleInputW(
                    input_handle,
                    event_buffer.as_mut_ptr(),
                    event_buffer.len() as _,
                    &mut event_count,
                ) == FALSE
                {
                    panic!("could not read console events");
                }

                for i in 0..event_count {
                    let event = event_buffer[i as usize];
                    match event.EventType {
                        KEY_EVENT => {
                            let event = event.Event.KeyEvent();
                            if event.bKeyDown == FALSE {
                                continue;
                            }

                            let control_key_state = event.dwControlKeyState;
                            let keycode = event.wVirtualKeyCode as i32;
                            let repeat_count = event.wRepeatCount as usize;

                            const CHAR_A: i32 = b'A' as _;
                            const CHAR_Z: i32 = b'Z' as _;
                            let key = match keycode {
                                VK_BACK => Key::Backspace,
                                VK_RETURN => Key::Enter,
                                VK_LEFT => Key::Left,
                                VK_RIGHT => Key::Right,
                                VK_UP => Key::Up,
                                VK_DOWN => Key::Down,
                                VK_HOME => Key::Home,
                                VK_END => Key::End,
                                VK_PRIOR => Key::PageUp,
                                VK_NEXT => Key::PageDown,
                                VK_TAB => Key::Tab,
                                VK_DELETE => Key::Delete,
                                VK_F1..=VK_F24 => Key::F((keycode - VK_F1 + 1) as _),
                                VK_ESCAPE => Key::Esc,
                                CHAR_A..=CHAR_Z => {
                                    const ALT_PRESSED_MASK: DWORD =
                                        LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED;
                                    const CTRL_PRESSED_MASK: DWORD =
                                        LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED;

                                    let c = keycode as u8;
                                    if control_key_state & ALT_PRESSED_MASK != 0 {
                                        Key::Alt(c.to_ascii_lowercase() as _)
                                    } else if control_key_state & CTRL_PRESSED_MASK != 0 {
                                        Key::Ctrl(c.to_ascii_lowercase() as _)
                                    } else if control_key_state & SHIFT_PRESSED != 0 {
                                        Key::Char(c as _)
                                    } else {
                                        Key::Char(c.to_ascii_lowercase() as _)
                                    }
                                }
                                _ => {
                                    let c = *(event.uChar.AsciiChar()) as u8;
                                    if !c.is_ascii_graphic() {
                                        continue;
                                    }

                                    Key::Char(c as _)
                                }
                            };

                            let message = format!("{}", key);
                            println!("{} key x {}", message, repeat_count);
                            match pipe.write(message.as_bytes()) {
                                WriteResult::Ok => (),
                                WriteResult::Err => panic!("could not send message to server"),
                            }

                            if let Key::Esc = key {
                                break 'main_loop;
                            }
                        }
                        WINDOW_BUFFER_SIZE_EVENT => {
                            let size = event.Event.WindowBufferSizeEvent().dwSize;
                            let x = size.X as u16;
                            let y = size.Y as u16;
                            println!("window resized to {}, {}", x, y);
                        }
                        _ => (),
                    }
                }
            }
            1 => match pipe.read_async() {
                ReadResult::Waiting => (),
                ReadResult::Ok(0) | ReadResult::Err => {
                    break;
                }
                ReadResult::Ok(len) => {
                    let buf = &[0u8];
                    let message = String::from_utf8_lossy(buf);
                    println!("received {} bytes from server! message: '{}'", len, message);
                }
            },
            _ => unreachable!(),
        }
    }

    println!("finish client");

    SetConsoleMode(input_handle, original_input_mode);
    SetConsoleMode(output_handle, original_output_mode);
}
