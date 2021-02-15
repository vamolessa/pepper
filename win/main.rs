use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    io,
    ops::{Deref, DerefMut},
    os::windows::io::AsRawHandle,
    process::{Child, Command},
    ptr::NonNull,
    sync::atomic::AtomicPtr,
    time::Duration,
};

use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, FALSE, TRUE},
        ntdef::NULL,
        winerror::{ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_CONNECTED, WAIT_TIMEOUT},
    },
    um::{
        consoleapi::{GetConsoleMode, ReadConsoleInputW, SetConsoleCtrlHandler, SetConsoleMode},
        errhandlingapi::GetLastError,
        fileapi::{CreateFileW, FindClose, FindFirstFileW, ReadFile, WriteFile, OPEN_EXISTING},
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        ioapiset::GetOverlappedResult,
        minwinbase::OVERLAPPED,
        namedpipeapi::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, SetNamedPipeHandleState,
        },
        processenv::{GetCommandLineW, GetCurrentDirectoryW, GetStdHandle},
        processthreadsapi::{CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW},
        stringapiset::{MultiByteToWideChar, WideCharToMultiByte},
        synchapi::{CreateEventW, SetEvent, WaitForMultipleObjects},
        winbase::{
            GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, FILE_FLAG_OVERLAPPED, GMEM_MOVEABLE,
            INFINITE, NORMAL_PRIORITY_CLASS, PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE,
            PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, STARTF_USESTDHANDLES, STD_INPUT_HANDLE,
            STD_OUTPUT_HANDLE, WAIT_OBJECT_0,
        },
        wincon::{
            GetConsoleScreenBufferInfo, ENABLE_PROCESSED_OUTPUT,
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
        },
        wincontypes::{
            INPUT_RECORD, KEY_EVENT, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED,
            RIGHT_CTRL_PRESSED, WINDOW_BUFFER_SIZE_EVENT,
        },
        winnls::CP_UTF8,
        winnt::{GENERIC_READ, GENERIC_WRITE, HANDLE, MAXIMUM_WAIT_OBJECTS},
        winuser::{
            CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
            CF_UNICODETEXT, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME,
            VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
        },
    },
};

use pepper::{
    application::{ClientApplication, ServerApplication},
    platform::{Key, Platform, PlatformWriter, RawPlatformWriter, ServerPlatformEvent},
    Args,
};

// max event count = 64
// 1 connection listener
// 13 connections
// 25 process stdout
// 25 process stderr
const MAX_CONNECTION_COUNT: usize = 13;
const MAX_PROCESS_COUNT: usize = 25;

const CLIENT_EVENT_BUFFER_LEN: usize = 32;

fn main() {
    let args = match Args::parse() {
        Some(args) => args,
        None => return,
    };

    let mut pipe_path = Vec::new();
    let mut hash_buf = [0u8; 16];
    let session_name = match args.session {
        Some(ref name) => name.as_str(),
        None => {
            use io::Write;
            get_current_directory(&mut pipe_path);
            let mut hasher = DefaultHasher::new();
            pipe_path.hash(&mut hasher);
            let current_directory_hash = hasher.finish();
            let mut cursor = io::Cursor::new(&mut hash_buf[..]);
            write!(&mut cursor, "{:x}", current_directory_hash).unwrap();
            let len = cursor.position() as usize;
            std::str::from_utf8(&hash_buf[..len]).unwrap()
        }
    };

    if args.print_session {
        println!("{}", session_name);
        return;
    }

    pipe_path.clear();
    pipe_path.extend("\\\\.\\pipe\\".encode_utf16());
    pipe_path.extend(session_name.encode_utf16());
    pipe_path.push(0);

    set_ctrlc_handler();

    let input_handle = get_std_handle(STD_INPUT_HANDLE);
    let output_handle = get_std_handle(STD_OUTPUT_HANDLE);

    match (input_handle, output_handle) {
        (Some(input_handle), Some(output_handle)) => {
            run_client(args, &pipe_path, input_handle, output_handle)
        }
        _ => run_server(args, &pipe_path),
    }
}

fn get_last_error() -> DWORD {
    unsafe { GetLastError() }
}

fn set_ctrlc_handler() {
    unsafe extern "system" fn ctrl_handler(_ctrl_type: DWORD) -> BOOL {
        FALSE
    }

    if unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) } == FALSE {
        panic!("could not set ctrl handler");
    }
}

fn get_current_directory(buf: &mut Vec<u16>) {
    let len = unsafe { GetCurrentDirectoryW(0, std::ptr::null_mut()) } as usize;
    buf.resize(len, 0);
    let len = unsafe { GetCurrentDirectoryW(buf.len() as _, buf.as_mut_ptr()) } as usize;
    if len == 0 {
        panic!("could not get current directory");
    }
    unsafe { buf.set_len(len) }
}

fn pipe_exists(path: &[u16]) -> bool {
    unsafe {
        let mut find_data = std::mem::zeroed();
        let find_handle = FindFirstFileW(path.as_ptr(), &mut find_data);
        if find_handle != INVALID_HANDLE_VALUE {
            FindClose(find_handle);
            true
        } else {
            false
        }
    }
}

fn get_std_handle(which: DWORD) -> Option<HANDLE> {
    let handle = unsafe { GetStdHandle(which) };
    if handle != NULL && handle != INVALID_HANDLE_VALUE {
        Some(handle)
    } else {
        None
    }
}

fn get_console_size(output_handle: HANDLE) -> (usize, usize) {
    let mut console_info = unsafe { std::mem::zeroed() };
    let result = unsafe { GetConsoleScreenBufferInfo(output_handle, &mut console_info) };
    if result == FALSE {
        panic!("could not get console info");
    }
    (console_info.dwSize.X as _, console_info.dwSize.Y as _)
}

fn read_console_input(input_handle: HANDLE, events: &mut [INPUT_RECORD]) -> &[INPUT_RECORD] {
    let mut event_count: DWORD = 0;
    let result = unsafe {
        ReadConsoleInputW(
            input_handle,
            events.as_mut_ptr(),
            events.len() as _,
            &mut event_count,
        )
    };
    if result == FALSE {
        panic!("could not read console events");
    }
    &events[..(event_count as usize)]
}

pub fn write_all_bytes(handle: HANDLE, mut buf: &[u8]) -> bool {
    while !buf.is_empty() {
        let mut write_len = 0;
        let result = unsafe {
            WriteFile(
                handle,
                buf.as_ptr() as _,
                buf.len() as _,
                &mut write_len,
                std::ptr::null_mut(),
            )
        };
        if result == FALSE {
            return false;
        }

        buf = &buf[(write_len as usize)..];
    }

    true
}

fn global_lock<T>(handle: HANDLE) -> Option<NonNull<T>> {
    NonNull::new(unsafe { GlobalLock(handle) as _ })
}

fn global_unlock(handle: HANDLE) {
    unsafe { GlobalUnlock(handle) };
}

fn fork() {
    let mut startup_info = unsafe { std::mem::zeroed::<STARTUPINFOW>() };
    startup_info.cb = std::mem::size_of::<STARTUPINFOW>() as _;
    startup_info.dwFlags = STARTF_USESTDHANDLES;
    startup_info.hStdInput = INVALID_HANDLE_VALUE;
    startup_info.hStdOutput = INVALID_HANDLE_VALUE;
    startup_info.hStdError = INVALID_HANDLE_VALUE;

    let mut process_info = unsafe { std::mem::zeroed::<PROCESS_INFORMATION>() };

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

fn wait_for_multiple_objects(handles: &[HANDLE], timeout: Option<Duration>) -> Option<usize> {
    let timeout = match timeout {
        Some(duration) => duration.as_millis() as _,
        None => INFINITE,
    };
    let len = MAXIMUM_WAIT_OBJECTS.min(handles.len() as DWORD);
    let result = unsafe { WaitForMultipleObjects(len, handles.as_ptr(), FALSE, timeout) };
    if result == WAIT_TIMEOUT {
        None
    } else if result >= WAIT_OBJECT_0 && result < (WAIT_OBJECT_0 + len) {
        Some((result - WAIT_OBJECT_0) as _)
    } else {
        panic!("could not wait for event")
    }
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
        let handle = unsafe { CreateEventW(std::ptr::null_mut(), TRUE, FALSE, std::ptr::null()) };
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

struct Clipboard;
impl Clipboard {
    pub fn open() {
        let result = unsafe { OpenClipboard(std::ptr::null_mut()) };
        if result == FALSE {
            panic!("could not open clipboard");
        }
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        let result = unsafe { CloseClipboard() };
        if result == FALSE {
            panic!("could not close clipboard");
        }
    }
}

struct Overlapped(OVERLAPPED);
impl Overlapped {
    pub fn with_event(event: &Event) -> Self {
        let mut overlapped = unsafe { std::mem::zeroed::<OVERLAPPED>() };
        overlapped.hEvent = event.handle();
        Self(overlapped)
    }

    pub fn as_mut_ptr(&mut self) -> *mut OVERLAPPED {
        &mut self.0
    }
}

struct ConsoleMode {
    console_handle: HANDLE,
    original_mode: DWORD,
}
impl ConsoleMode {
    pub fn new(console_handle: HANDLE) -> Self {
        let mut original_mode = DWORD::default();
        let result = unsafe { GetConsoleMode(console_handle, &mut original_mode) };
        if result == FALSE {
            panic!("could not get console mode");
        }
        Self {
            console_handle,
            original_mode,
        }
    }

    pub fn set(&self, mode: DWORD) {
        let result = unsafe { SetConsoleMode(self.console_handle, mode) };
        if result == FALSE {
            panic!("could not set console mode");
        }
    }
}
impl Drop for ConsoleMode {
    fn drop(&mut self) {
        self.set(self.original_mode);
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
}
impl AsyncIO {
    pub fn from_handle(handle: Handle, buf_len: usize) -> Self {
        let event = Event::new();
        event.notify();
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
        }
    }

    pub fn read_async(&mut self) -> ReadResult {
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
                        ReadResult::Ok(read_len as _)
                    }
                    _ => {
                        self.pending_io = false;
                        ReadResult::Err
                    }
                }
            } else {
                self.pending_io = false;
                self.event.notify();
                ReadResult::Ok(read_len as _)
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
                ReadResult::Ok(read_len as _)
            }
        }
    }

    pub fn get_bytes(&self, len: usize) -> &[u8] {
        &self.buf[..len]
    }

    pub fn write(&mut self, buf: &[u8]) -> bool {
        write_all_bytes(self.handle.0, buf)
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
    pub fn connect(path: &[u16], buf_len: usize) -> Self {
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
        let io = AsyncIO::from_handle(pipe_handle, buf_len);
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
    pub fn new(pipe_path: &[u16], buf_len: usize) -> Self {
        let pipe_handle = unsafe {
            CreateNamedPipeW(
                pipe_path.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE,
                PIPE_UNLIMITED_INSTANCES,
                buf_len as _,
                buf_len as _,
                0,
                std::ptr::null_mut(),
            )
        };
        if pipe_handle == INVALID_HANDLE_VALUE {
            panic!("could not create new connection");
        }

        let pipe_handle = Handle(pipe_handle);
        let mut pipe = AsyncIO::from_handle(pipe_handle, buf_len);

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

    pub fn accept(&mut self, pipe_path: &[u16], buf_len: usize) -> Option<PipeToClient> {
        match self.io.read_async() {
            ReadResult::Waiting => None,
            ReadResult::Ok(_) => {
                let mut io = Self::new(pipe_path, buf_len).io;
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
    pub fn from_handle(handle: Option<Handle>, buf_len: usize) -> Self {
        match handle {
            Some(h) => {
                let io = AsyncIO::from_handle(h, buf_len);
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
    pub fn from_child(child: Child, stdout_buf_len: usize, stderr_buf_len: usize) -> Self {
        let stdout = ChildPipe::from_handle(
            child
                .stdout
                .as_ref()
                .map(|s| Handle(s.as_raw_handle() as _)),
            stdout_buf_len,
        );
        let stderr = ChildPipe::from_handle(
            child
                .stderr
                .as_ref()
                .map(|s| Handle(s.as_raw_handle() as _)),
            stderr_buf_len,
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
            Some(index) => Some(self.sources.swap_remove(index)),
            None => None,
        };

        self.wait_handles.clear();
        self.sources.clear();
        result
    }
}

const NULL_ATOMIC_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static CONNECTION_HANDLES: [AtomicPtr<()>; MAX_CONNECTION_COUNT] =
    [NULL_ATOMIC_PTR; MAX_CONNECTION_COUNT];
static PROCESS_STDIN_HANDLES: [AtomicPtr<()>; MAX_PROCESS_COUNT] =
    [NULL_ATOMIC_PTR; MAX_PROCESS_COUNT];

struct ServerState {
    pipes: [Option<PipeToClient>; MAX_CONNECTION_COUNT],
    pipes_len: usize,
    children: [Option<AsyncChild>; MAX_PROCESS_COUNT],
    children_len: usize,
}

impl Platform for ServerState {
    fn read_from_clipboard(&mut self, text: &mut String) -> bool {
        let clipboard = Clipboard::open();
        text.clear();
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if handle == NULL {
            return false;
        }
        let data = match global_lock::<u16>(handle) {
            Some(data) => data,
            None => return false,
        };
        let data = data.as_ptr();
        let len = unsafe {
            WideCharToMultiByte(
                CP_UTF8,
                0,
                data,
                -1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if len != 0 {
            let len = len - 1;
            let mut temp = String::new();
            std::mem::swap(text, &mut temp);
            let mut bytes = temp.into_bytes();
            bytes.resize(len as usize, 0);

            unsafe {
                WideCharToMultiByte(
                    CP_UTF8,
                    0,
                    data,
                    -1,
                    bytes.as_mut_ptr() as _,
                    bytes.len() as _,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                );
            }

            temp = unsafe { String::from_utf8_unchecked(bytes) };
            std::mem::swap(text, &mut temp);
        }
        global_unlock(handle);
        drop(clipboard);
        true
    }

    fn write_to_clipboard(&mut self, text: &str) {
        let clipboard = Clipboard::open();
        let len = unsafe {
            MultiByteToWideChar(
                CP_UTF8,
                0,
                text.as_ptr() as _,
                text.len() as _,
                std::ptr::null_mut(),
                0,
            )
        };
        if len != 0 {
            let size = (len as usize + 1) * std::mem::size_of::<u16>();
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size as _) };
            if handle == NULL {
                return;
            }
            let data = match global_lock::<u16>(handle) {
                Some(data) => data.as_ptr(),
                None => {
                    unsafe { GlobalFree(handle) };
                    return;
                }
            };
            unsafe {
                MultiByteToWideChar(CP_UTF8, 0, text.as_ptr() as _, text.len() as _, data, len);
                std::ptr::write(data.offset(len as isize), 0);
            }
            global_unlock(handle);

            unsafe { EmptyClipboard() };
            let result = unsafe { SetClipboardData(CF_UNICODETEXT, handle) };
            if result == NULL {
                unsafe { GlobalFree(handle) };
            }
        }
        drop(clipboard);
    }

    fn read_from_connection(&mut self, index: usize, len: usize) -> &[u8] {
        if index >= self.pipes_len {
            return &[];
        }
        match self.pipes[index] {
            Some(ref pipe) => pipe.get_bytes(len),
            None => &[],
        }
    }

    fn write_to_connection(&mut self, index: usize, buf: &[u8]) -> bool {
        if index >= self.pipes_len {
            return false;
        }
        match self.pipes[index] {
            Some(ref mut pipe) => pipe.write(buf),
            None => false,
        }
    }

    fn close_connection(&mut self, index: usize) {
        if index < self.pipes_len {
            self.pipes[index] = None;
        }
    }

    fn spawn_process(
        &mut self,
        mut command: Command,
        stdout_buf_len: usize,
        stderr_buf_len: usize,
    ) -> io::Result<usize> {
        let index = self.children_len;
        if index >= self.children.len() {
            return Err(io::Error::from(io::ErrorKind::Other));
        }

        self.children[index] = Some(AsyncChild::from_child(
            command.spawn()?,
            stdout_buf_len,
            stderr_buf_len,
        ));
        self.children_len += 1;

        Ok(index)
    }

    fn read_from_process_stdout(&mut self, index: usize, len: usize) -> &[u8] {
        if index >= self.children_len {
            return &[];
        }
        match self.children[index] {
            Some(ref child) => match child.stdout {
                ChildPipe::Open(ref io) => io.get_bytes(len),
                ChildPipe::Closed => &[],
            },
            None => &[],
        }
    }

    fn read_from_process_stderr(&mut self, index: usize, len: usize) -> &[u8] {
        if index >= self.children_len {
            return &[];
        }
        match self.children[index] {
            Some(ref child) => match child.stderr {
                ChildPipe::Open(ref io) => io.get_bytes(len),
                ChildPipe::Closed => &[],
            },
            None => &[],
        }
    }

    fn write_to_process(&mut self, index: usize, buf: &[u8]) -> bool {
        if index >= self.children_len {
            return false;
        }
        if let Some(ref mut child) = self.children[index] {
            if let Some(ref mut stdin) = child.child.stdin {
                use io::Write;
                if let Ok(()) = stdin.write_all(buf) {
                    return true;
                }
            }
        }

        false
    }

    fn kill_process(&mut self, index: usize) {
        if index >= self.children_len {
            return;
        }
        if let Some(ref mut child) = self.children[index] {
            let _ = child.child.kill();
            let _ = child.child.wait();
            self.children[index] = None;
        }
    }
}

fn run_server(args: Args, pipe_path: &[u16]) {
    if pipe_exists(pipe_path) {
        return;
    }

    let connection_buffer_len = ServerApplication::connection_buffer_len();
    let mut events = Events::default();
    let mut listener = PipeToClientListener::new(pipe_path, connection_buffer_len);
    let mut state = ServerState {
        pipes: Default::default(),
        pipes_len: 0,
        children: Default::default(),
        children_len: 0,
    };

    let mut application = match ServerApplication::new(args, &mut state) {
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
        events.track(&listener.io.event, EventSource::ConnectionListener);
        for i in 0..state.pipes_len {
            if let Some(pipe) = &state.pipes[i] {
                events.track(&pipe.event, EventSource::Connection(i));
            }
        }
        for i in 0..state.children_len {
            if let Some(child) = &state.children[i] {
                if let ChildPipe::Open(io) = &child.stdout {
                    events.track(&io.event, EventSource::ChildStdout(i));
                }
                if let ChildPipe::Open(io) = &child.stderr {
                    events.track(&io.event, EventSource::ChildStderr(i));
                }
            }
        }

        match events.wait_one(None) {
            Some(EventSource::ConnectionListener) => {
                if let Some(pipe) = listener.accept(pipe_path, connection_buffer_len) {
                    if state.pipes_len < state.pipes.len() {
                        let index = state.pipes_len;
                        state.pipes_len += 1;
                        state.pipes[index] = Some(pipe);
                        send_event!(ServerPlatformEvent::ConnectionOpen { index });
                    }
                }
            }
            Some(EventSource::Connection(index)) => {
                if index >= state.pipes.len() {
                    continue;
                }
                let pipe = match state.pipes[index] {
                    Some(ref mut pipe) => pipe,
                    None => continue,
                };

                match pipe.read_async() {
                    ReadResult::Waiting => (),
                    ReadResult::Err | ReadResult::Ok(0) => {
                        state.pipes[index] = None;
                        send_event!(ServerPlatformEvent::ConnectionClose { index });
                    }
                    ReadResult::Ok(len) => {
                        send_event!(ServerPlatformEvent::ConnectionMessage { index, len });
                    }
                }
            }
            Some(EventSource::ChildStdout(index)) => {
                if index >= state.children.len() {
                    continue;
                }
                let child = match state.children[index] {
                    Some(ref mut child) => child,
                    None => continue,
                };
                let io = match child.stdout {
                    ChildPipe::Open(ref mut io) => io,
                    ChildPipe::Closed => continue,
                };

                match io.read_async() {
                    ReadResult::Waiting => (),
                    ReadResult::Err | ReadResult::Ok(0) => {
                        child.stdout = ChildPipe::Closed;
                        if let ChildPipe::Closed = child.stderr {
                            let success = child.child.wait().unwrap().success();
                            state.children[index] = None;
                            send_event!(ServerPlatformEvent::ProcessExit { index, success });
                        }
                    }
                    ReadResult::Ok(len) => {
                        send_event!(ServerPlatformEvent::ProcessStdout { index, len });
                    }
                }
            }
            Some(EventSource::ChildStderr(index)) => {
                if index >= state.children.len() {
                    continue;
                }
                let child = match state.children[index] {
                    Some(ref mut child) => child,
                    None => continue,
                };
                let io = match child.stderr {
                    ChildPipe::Open(ref mut io) => io,
                    ChildPipe::Closed => continue,
                };

                match io.read_async() {
                    ReadResult::Waiting => (),
                    ReadResult::Err | ReadResult::Ok(0) => {
                        child.stderr = ChildPipe::Closed;
                        if let ChildPipe::Closed = child.stdout {
                            let success = child.child.wait().unwrap().success();
                            state.children[index] = None;
                            send_event!(ServerPlatformEvent::ProcessExit { index, success });
                        }
                    }
                    ReadResult::Ok(len) => {
                        send_event!(ServerPlatformEvent::ProcessStderr { index, len });
                    }
                }
            }
            None => panic!("timeout waiting"),
        }
    }
}

fn run_client(args: Args, pipe_path: &[u16], input_handle: HANDLE, output_handle: HANDLE) {
    if !pipe_exists(pipe_path) {
        fork();
        while !pipe_exists(pipe_path) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    let mut pipe = PipeToServer::connect(pipe_path, ClientApplication::connection_buffer_len());
    let mut application = ClientApplication::new();
    pipe.write(application.init(args));

    let console_input_mode = ConsoleMode::new(input_handle);
    console_input_mode.set(ENABLE_WINDOW_INPUT);
    let console_output_mode = ConsoleMode::new(output_handle);
    console_output_mode.set(ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING);

    let mut event_buffer = [unsafe { std::mem::zeroed() }; CLIENT_EVENT_BUFFER_LEN];
    let wait_handles = [pipe.event.handle(), input_handle];

    let mut keys = Vec::new();

    let (width, height) = get_console_size(output_handle);
    let bytes = application.update(Some((width, height)), &[], &[]);
    if bytes.is_empty() {
        return;
    }
    pipe.write(bytes);

    loop {
        let wait_handle_index = match wait_for_multiple_objects(&wait_handles, None) {
            Some(i) => i,
            _ => continue,
        };

        let mut resize = None;
        keys.clear();
        let mut message = &[][..];

        match wait_handle_index {
            0 => match pipe.read_async() {
                ReadResult::Waiting => (),
                ReadResult::Ok(0) | ReadResult::Err => break,
                ReadResult::Ok(len) => message = pipe.get_bytes(len),
            },
            1 => {
                let events = read_console_input(input_handle, &mut event_buffer);
                for event in events {
                    match event.EventType {
                        KEY_EVENT => {
                            let event = unsafe { event.Event.KeyEvent() };
                            if event.bKeyDown == FALSE {
                                continue;
                            }

                            let control_key_state = event.dwControlKeyState;
                            let keycode = event.wVirtualKeyCode as i32;
                            let unicode_char = unsafe { *event.uChar.UnicodeChar() };
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
                                VK_SPACE => {
                                    match std::char::decode_utf16(std::iter::once(unicode_char))
                                        .next()
                                    {
                                        Some(Ok(c)) => Key::Char(c),
                                        _ => continue,
                                    }
                                }
                                CHAR_A..=CHAR_Z => {
                                    const ALT_PRESSED_MASK: DWORD =
                                        LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED;
                                    const CTRL_PRESSED_MASK: DWORD =
                                        LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED;

                                    if control_key_state & ALT_PRESSED_MASK != 0 {
                                        let c = (keycode - CHAR_A) as u8 + b'a';
                                        Key::Alt(c.to_ascii_lowercase() as _)
                                    } else if control_key_state & CTRL_PRESSED_MASK != 0 {
                                        let c = (keycode - CHAR_A) as u8 + b'a';
                                        Key::Ctrl(c.to_ascii_lowercase() as _)
                                    } else {
                                        match std::char::decode_utf16(std::iter::once(unicode_char))
                                            .next()
                                        {
                                            Some(Ok(c)) => Key::Char(c),
                                            _ => continue,
                                        }
                                    }
                                }
                                _ => match std::char::decode_utf16(std::iter::once(unicode_char))
                                    .next()
                                {
                                    Some(Ok(c)) if c.is_ascii_graphic() => Key::Char(c),
                                    _ => continue,
                                },
                            };

                            for _ in 0..repeat_count {
                                keys.push(key);
                            }
                        }
                        WINDOW_BUFFER_SIZE_EVENT => {
                            let size = unsafe { event.Event.WindowBufferSizeEvent().dwSize };
                            resize = Some((size.X as _, size.Y as _));
                        }
                        _ => (),
                    }
                }
            }
            _ => unreachable!(),
        }

        let bytes = application.update(resize, &keys, message);
        if bytes.is_empty() {
            break;
        }
        pipe.write(bytes);
    }
}
