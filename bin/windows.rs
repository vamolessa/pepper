use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    io,
    os::windows::io::IntoRawHandle,
    process::{Child, ChildStdin},
    ptr::NonNull,
    sync::{
        atomic::{AtomicPtr, Ordering},
        mpsc,
    },
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
        fileapi::{
            CreateFileW, FindClose, FindFirstFileW, GetFileType, ReadFile, WriteFile, OPEN_EXISTING,
        },
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
            GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, FILE_FLAG_OVERLAPPED,
            FILE_TYPE_CHAR, GMEM_MOVEABLE, INFINITE, NORMAL_PRIORITY_CLASS, PIPE_ACCESS_DUPLEX,
            PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, STARTF_USESTDHANDLES,
            STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, WAIT_OBJECT_0,
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
    application::{AnyError, ApplicationEvent, ClientApplication, ProcessTag, ServerApplication},
    client::ClientHandle,
    platform::{BufPool, ExclusiveBuf, Key, Platform, PlatformRequest, ProcessHandle, SharedBuf},
    Args,
};

const MAX_CLIENT_COUNT: usize = 12;
const MAX_PROCESS_COUNT: usize = 25;
const MAX_EVENT_COUNT: usize = 1 + 1 + MAX_CLIENT_COUNT + 2 * MAX_PROCESS_COUNT;
const _ASSERT_MAX_EVENT_COUNT_IS_64: [(); 64] = [(); MAX_EVENT_COUNT];

const CLIENT_CONSOLE_EVENT_BUFFER_LEN: usize = 32;

pub fn main() {
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
            run_client(args, &pipe_path, input_handle, output_handle);
        }
        _ => {
            let _ = run_server(args, &pipe_path);
        }
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

enum ReadResult {
    Waiting,
    Ok(usize),
    Err,
}

struct AsyncReader {
    handle: Handle,
    event: Event,
    overlapped: Overlapped,
    pending_io: bool,
}
impl AsyncReader {
    pub fn new(handle: Handle) -> Self {
        let event = Event::manual();
        event.notify();
        let overlapped = Overlapped::with_event(&event);

        Self {
            handle,
            event,
            overlapped,
            pending_io: false,
        }
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn event(&self) -> &Event {
        &self.event
    }

    pub fn overlapped(&mut self) -> &mut Overlapped {
        &mut self.overlapped
    }

    pub fn read_async(&mut self, buf: &mut [u8]) -> ReadResult {
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

            self.pending_io = false;

            if result == FALSE {
                match get_last_error() {
                    ERROR_MORE_DATA => {
                        self.event.notify();
                        ReadResult::Ok(read_len as _)
                    }
                    _ => ReadResult::Err,
                }
            } else {
                self.event.notify();
                ReadResult::Ok(read_len as _)
            }
        } else {
            let result = unsafe {
                ReadFile(
                    self.handle.0,
                    buf.as_mut_ptr() as _,
                    buf.len() as _,
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
                    _ => ReadResult::Err,
                }
            } else {
                self.event.notify();
                ReadResult::Ok(read_len as _)
            }
        }
    }
}

fn is_pipped(handle: HANDLE) -> bool {
    unsafe { GetFileType(handle) != FILE_TYPE_CHAR }
}

fn read_all_bytes(handle: &Handle, mut buf: &mut [u8]) -> usize {
    let mut total_read = 0;
    while !buf.is_empty() {
        let mut read_len = 0;
        let result = unsafe {
            ReadFile(
                handle.0,
                buf.as_ptr() as _,
                buf.len() as _,
                &mut read_len,
                std::ptr::null_mut(),
            )
        };
        if result == FALSE || read_len == 0 {
            break;
        }

        let read_len = read_len as usize;
        buf = &mut buf[read_len..];
        total_read += read_len;
    }

    total_read
}

fn write_all_bytes(handle: &Handle, mut buf: &[u8]) -> bool {
    while !buf.is_empty() {
        let mut write_len = 0;
        let result = unsafe {
            WriteFile(
                handle.0,
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

fn read_from_clipboard(text: &mut String) -> bool {
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

fn write_to_clipboard(text: &str) {
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

struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn create_event(manual_reset: bool, initial_state: bool) -> HANDLE {
    let manual_reset = if manual_reset { TRUE } else { FALSE };
    let initial_state = if initial_state { TRUE } else { FALSE };
    let handle = unsafe {
        CreateEventW(
            std::ptr::null_mut(),
            manual_reset,
            initial_state,
            std::ptr::null(),
        )
    };
    if handle == NULL {
        panic!("could not create event");
    }
    handle
}

fn set_event(handle: HANDLE) {
    if unsafe { SetEvent(handle) } == FALSE {
        panic!("could not set event");
    }
}

struct Event(HANDLE);
impl Event {
    pub fn automatic() -> Self {
        Self(create_event(false, false))
    }

    pub fn manual() -> Self {
        Self(create_event(true, false))
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    pub fn notify(&self) {
        set_event(self.0);
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

struct ConnectionToClient {
    reader: AsyncReader,
    current_read_buf: Option<ExclusiveBuf>,
}
impl ConnectionToClient {
    pub fn new(reader: AsyncReader) -> Self {
        Self {
            reader,
            current_read_buf: None,
        }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn read_async(
        &mut self,
        buf_len: usize,
        buf_pool: &mut BufPool,
    ) -> Result<Option<SharedBuf>, ()> {
        let mut read_buf = match self.current_read_buf.take() {
            Some(buf) => buf,
            None => buf_pool.acquire(),
        };
        let buf = read_buf.write();
        buf.resize(buf_len, 0);

        match self.reader.read_async(buf) {
            ReadResult::Waiting => {
                self.current_read_buf = Some(read_buf);
                Ok(None)
            }
            ReadResult::Ok(len) => {
                buf.truncate(len);
                Ok(Some(read_buf.share()))
            }
            ReadResult::Err => Err(()),
        }
    }

    pub fn write(&self, buf: &[u8]) -> bool {
        write_all_bytes(self.reader.handle(), buf)
    }
}
impl Drop for ConnectionToClient {
    fn drop(&mut self) {
        unsafe { DisconnectNamedPipe(self.reader.handle().0) };
    }
}

struct ConnectionToClientListener {
    reader: AsyncReader,
    buf: Box<[u8]>,
}
impl ConnectionToClientListener {
    fn new_listen_reader(pipe_path: &[u16], buf_len: usize) -> AsyncReader {
        let handle = unsafe {
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
        if handle == INVALID_HANDLE_VALUE {
            panic!("could not create new connection");
        }

        let mut reader = AsyncReader::new(Handle(handle));

        if unsafe { ConnectNamedPipe(reader.handle().0, reader.overlapped().as_mut_ptr()) } != FALSE
        {
            panic!("could not accept incomming connection");
        }

        reader.pending_io = match get_last_error() {
            ERROR_IO_PENDING => true,
            ERROR_PIPE_CONNECTED => {
                reader.event.notify();
                false
            }
            _ => panic!("could not accept incomming connection"),
        };
        reader.overlapped = Overlapped::with_event(reader.event());

        reader
    }

    pub fn new(pipe_path: &[u16], buf_len: usize) -> Self {
        let reader = Self::new_listen_reader(pipe_path, buf_len);

        let mut buf = Vec::with_capacity(buf_len);
        buf.resize(buf_len, 0);
        let buf = buf.into_boxed_slice();

        Self { reader, buf }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn accept(&mut self, pipe_path: &[u16]) -> Option<ConnectionToClient> {
        match self.reader.read_async(&mut self.buf) {
            ReadResult::Waiting => None,
            ReadResult::Ok(_) => {
                let mut reader = Self::new_listen_reader(pipe_path, self.buf.len());
                std::mem::swap(&mut reader, &mut self.reader);
                Some(ConnectionToClient::new(reader))
            }
            ReadResult::Err => panic!("could not accept connection {}", get_last_error()),
        }
    }
}

struct ConnectionToServer {
    reader: AsyncReader,
    buf: Box<[u8]>,
}
impl ConnectionToServer {
    pub fn connect(path: &[u16], buf_len: usize) -> Self {
        let handle = unsafe {
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
        if handle == INVALID_HANDLE_VALUE {
            panic!("could not establish a connection {}", get_last_error());
        }

        let mut mode = PIPE_READMODE_BYTE;
        let result = unsafe {
            SetNamedPipeHandleState(
                handle,
                &mut mode,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };

        if result == FALSE {
            panic!("could not establish a connection");
        }

        let reader = AsyncReader::new(Handle(handle));
        let mut buf = Vec::with_capacity(buf_len);
        buf.resize(buf_len, 0);
        let buf = buf.into_boxed_slice();

        Self { reader, buf }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn write(&mut self, buf: &[u8]) -> bool {
        write_all_bytes(self.reader.handle(), buf)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        read_all_bytes(self.reader.handle(), buf)
    }

    pub fn read_async(&mut self) -> Result<&[u8], ()> {
        match self.reader.read_async(&mut self.buf) {
            ReadResult::Waiting => Ok(&[]),
            ReadResult::Ok(0) | ReadResult::Err => Err(()),
            ReadResult::Ok(len) => Ok(&self.buf[..len]),
        }
    }
}

struct ProcessPipe {
    reader: AsyncReader,
    buf_len: usize,
    current_read_buf: Option<ExclusiveBuf>,
}
impl ProcessPipe {
    pub fn new(reader: AsyncReader, buf_len: usize) -> Self {
        reader.event.notify();

        Self {
            reader,
            buf_len,
            current_read_buf: None,
        }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn read_async(&mut self, buf_pool: &mut BufPool) -> Result<Option<SharedBuf>, ()> {
        let mut read_buf = match self.current_read_buf.take() {
            Some(buf) => buf,
            None => buf_pool.acquire(),
        };
        let buf = read_buf.write();
        buf.resize(self.buf_len, 0);

        match self.reader.read_async(buf) {
            ReadResult::Waiting => {
                self.current_read_buf = Some(read_buf);
                Ok(None)
            }
            ReadResult::Ok(len) => {
                buf.truncate(len);
                Ok(Some(read_buf.share()))
            }
            ReadResult::Err => Err(()),
        }
    }
}

struct AsyncProcess {
    child: Child,
    tag: ProcessTag,
    pub stdout: Option<ProcessPipe>,
    pub stderr: Option<ProcessPipe>,
}
impl AsyncProcess {
    pub fn new(
        mut child: Child,
        tag: ProcessTag,
        stdout_buf_len: usize,
        stderr_buf_len: usize,
    ) -> Self {
        fn raw_handle_to_pipe(handle: HANDLE, buf_len: usize) -> ProcessPipe {
            let reader = AsyncReader::new(Handle(handle));
            ProcessPipe::new(reader, buf_len)
        }

        let stdout = child
            .stdout
            .take()
            .map(IntoRawHandle::into_raw_handle)
            .map(|h| raw_handle_to_pipe(h as _, stdout_buf_len));

        let stderr = child
            .stderr
            .take()
            .map(IntoRawHandle::into_raw_handle)
            .map(|h| raw_handle_to_pipe(h as _, stderr_buf_len));

        Self {
            child,
            tag,
            stdout,
            stderr,
        }
    }

    pub fn stdin(&mut self) -> Option<&mut ChildStdin> {
        self.child.stdin.as_mut()
    }

    pub fn wait(&mut self) -> bool {
        self.stdout = None;
        self.stderr = None;
        match self.child.wait() {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    pub fn kill(&mut self) {
        self.stdout = None;
        self.stderr = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

enum EventSource {
    NewRequest,
    ConnectionListener,
    Connection(usize),
    ProcessStdout(usize),
    ProcessStderr(usize),
}
struct Events {
    wait_handles: [HANDLE; MAX_EVENT_COUNT],
    sources: [EventSource; MAX_EVENT_COUNT],
    len: usize,
}
impl Events {
    pub fn new() -> Self {
        const DEFAULT_EVENT_SOURCE: EventSource = EventSource::ConnectionListener;

        Self {
            wait_handles: [NULL; MAX_EVENT_COUNT],
            sources: [DEFAULT_EVENT_SOURCE; MAX_EVENT_COUNT],
            len: 0,
        }
    }

    pub fn track(&mut self, event: &Event, source: EventSource) {
        let index = self.len;
        debug_assert!(index < MAX_EVENT_COUNT);
        self.wait_handles[index] = event.handle();
        self.sources[index] = source;
        self.len += 1;
    }

    pub fn wait_next(&mut self, timeout: Option<Duration>) -> Option<EventSource> {
        let len = self.len;
        self.len = 0;
        match wait_for_multiple_objects(&self.wait_handles[..len], timeout) {
            Some(index) => {
                let mut source = EventSource::ConnectionListener;
                std::mem::swap(&mut source, &mut self.sources[index]);
                Some(source)
            }
            None => None,
        }
    }
}

static NEW_REQUEST_EVENT_HANDLE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

fn run_server(args: Args, pipe_path: &[u16]) -> Result<(), AnyError> {
    if pipe_exists(pipe_path) {
        return Ok(());
    }

    let mut events = Events::new();
    let mut listener =
        ConnectionToClientListener::new(pipe_path, ServerApplication::connection_buffer_len());

    let mut client_connections: [Option<ConnectionToClient>; MAX_CLIENT_COUNT] = Default::default();
    let mut processes: [Option<AsyncProcess>; MAX_PROCESS_COUNT] = Default::default();
    let mut buf_pool = BufPool::default();

    let new_request_event = Event::automatic();
    NEW_REQUEST_EVENT_HANDLE.store(new_request_event.0 as _, Ordering::Relaxed);

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(
        read_from_clipboard,
        write_to_clipboard,
        || set_event(NEW_REQUEST_EVENT_HANDLE.load(Ordering::Relaxed) as _),
        request_sender,
    );

    let event_sender = match ServerApplication::run(args, platform) {
        Some(sender) => sender,
        None => return Ok(()),
    };

    loop {
        events.track(&new_request_event, EventSource::NewRequest);
        events.track(listener.event(), EventSource::ConnectionListener);
        for (i, connection) in client_connections.iter().enumerate() {
            if let Some(connection) = connection {
                events.track(connection.event(), EventSource::Connection(i));
            }
        }
        for (i, process) in processes.iter().enumerate() {
            if let Some(process) = process {
                if let Some(ref stdout) = process.stdout {
                    events.track(stdout.event(), EventSource::ProcessStdout(i));
                }
                if let Some(ref stderr) = process.stderr {
                    events.track(stderr.event(), EventSource::ProcessStderr(i));
                }
            }
        }

        match events.wait_next(None) {
            Some(EventSource::NewRequest) => {
                for request in request_receiver.try_iter() {
                    match request {
                        PlatformRequest::Exit => return Ok(()),
                        PlatformRequest::WriteToClient { handle, buf } => {
                            if let Some(ref mut connection) =
                                client_connections[handle.into_index()]
                            {
                                if !connection.write(buf.as_bytes()) {
                                    client_connections[handle.into_index()] = None;
                                    event_sender
                                        .send(ApplicationEvent::ConnectionClose { handle })?;
                                }
                            }
                        }
                        PlatformRequest::CloseClient { handle } => {
                            client_connections[handle.into_index()] = None;
                            event_sender.send(ApplicationEvent::ConnectionClose { handle })?;
                        }
                        PlatformRequest::SpawnProcess {
                            tag,
                            mut command,
                            stdout_buf_len,
                            stderr_buf_len,
                        } => {
                            for (i, p) in processes.iter_mut().enumerate() {
                                if p.is_some() {
                                    continue;
                                }

                                let handle = ProcessHandle(i);
                                match command.spawn() {
                                    Ok(child) => {
                                        *p = Some(AsyncProcess::new(
                                            child,
                                            tag,
                                            stdout_buf_len,
                                            stderr_buf_len,
                                        ));
                                        event_sender.send(ApplicationEvent::ProcessSpawned {
                                            tag,
                                            handle,
                                        })?;
                                    }
                                    Err(_) => event_sender.send(ApplicationEvent::ProcessExit {
                                        tag,
                                        success: false,
                                    })?,
                                }
                                break;
                            }
                        }
                        PlatformRequest::WriteToProcess { handle, buf } => {
                            if let Some(ref mut process) = processes[handle.0] {
                                if let Some(pipe) = process.stdin() {
                                    use io::Write;
                                    if pipe.write_all(buf.as_bytes()).is_err() {
                                        let tag = process.tag;
                                        process.kill();
                                        processes[handle.0] = None;
                                        event_sender.send(ApplicationEvent::ProcessExit {
                                            tag,
                                            success: false,
                                        })?;
                                    }
                                }
                            }
                        }
                        PlatformRequest::KillProcess { handle } => {
                            if let Some(ref mut process) = processes[handle.0] {
                                let tag = process.tag;
                                process.kill();
                                processes[handle.0] = None;
                                event_sender.send(ApplicationEvent::ProcessExit {
                                    tag,
                                    success: false,
                                })?;
                            }
                        }
                    }
                }
            }
            Some(EventSource::ConnectionListener) => {
                if let Some(connection) = listener.accept(pipe_path) {
                    for (i, c) in client_connections.iter_mut().enumerate() {
                        if c.is_none() {
                            *c = Some(connection);
                            let handle = ClientHandle::from_index(i).unwrap();
                            event_sender.send(ApplicationEvent::ConnectionOpen { handle })?;
                            break;
                        }
                    }
                }
            }
            Some(EventSource::Connection(i)) => {
                if let Some(ref mut connection) = client_connections[i] {
                    let handle = ClientHandle::from_index(i).unwrap();
                    match connection
                        .read_async(ServerApplication::connection_buffer_len(), &mut buf_pool)
                    {
                        Ok(None) => (),
                        Ok(Some(buf)) => event_sender
                            .send(ApplicationEvent::ConnectionMessage { handle, buf })?,
                        Err(()) => {
                            client_connections[i] = None;
                            event_sender.send(ApplicationEvent::ConnectionClose { handle })?;
                        }
                    }
                }
            }
            Some(EventSource::ProcessStdout(i)) => {
                if let Some(ref mut process) = processes[i] {
                    if let Some(ref mut pipe) = process.stdout {
                        match pipe.read_async(&mut buf_pool) {
                            Ok(None) => (),
                            Ok(Some(buf)) => {
                                event_sender.send(ApplicationEvent::ProcessStdout {
                                    tag: process.tag,
                                    buf,
                                })?
                            }
                            Err(()) => {
                                process.stdout = None;
                                if process.stderr.is_none() {
                                    let tag = process.tag;
                                    let success = process.wait();
                                    processes[i] = None;
                                    event_sender
                                        .send(ApplicationEvent::ProcessExit { tag, success })?;
                                }
                            }
                        }
                    }
                }
            }
            Some(EventSource::ProcessStderr(i)) => {
                if let Some(ref mut process) = processes[i] {
                    if let Some(ref mut pipe) = process.stderr {
                        match pipe.read_async(&mut buf_pool) {
                            Ok(None) => (),
                            Ok(Some(buf)) => {
                                event_sender.send(ApplicationEvent::ProcessStdout {
                                    tag: process.tag,
                                    buf,
                                })?
                            }
                            Err(()) => {
                                process.stderr = None;
                                if process.stdout.is_none() {
                                    let tag = process.tag;
                                    let success = process.wait();
                                    processes[i] = None;
                                    event_sender
                                        .send(ApplicationEvent::ProcessExit { tag, success })?;
                                }
                            }
                        }
                    }
                }
            }
            _ => unreachable!(),
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

    let mut connection =
        ConnectionToServer::connect(pipe_path, ClientApplication::connection_buffer_len());

    let mut client_index = 0;
    if connection.read(std::slice::from_mut(&mut client_index)) == 0 {
        return;
    }

    let client_handle = ClientHandle::from_index(client_index as _).unwrap();
    let mut application = ClientApplication::new(client_handle);

    let is_pipped = is_pipped(input_handle);
    let bytes = application.init(args, is_pipped);
    if !connection.write(bytes) || is_pipped {
        return;
    }

    let console_input_mode = ConsoleMode::new(input_handle);
    console_input_mode.set(ENABLE_WINDOW_INPUT);
    let console_output_mode = ConsoleMode::new(output_handle);
    console_output_mode.set(ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING);

    let mut console_event_buf = [unsafe { std::mem::zeroed() }; CLIENT_CONSOLE_EVENT_BUFFER_LEN];
    let mut keys = Vec::with_capacity(CLIENT_CONSOLE_EVENT_BUFFER_LEN);
    let wait_handles = [connection.event().handle(), input_handle];

    let (width, height) = get_console_size(output_handle);
    let bytes = application.update(Some((width, height)), &[], &[]);
    if !connection.write(bytes) {
        return;
    }

    loop {
        let wait_handle_index = match wait_for_multiple_objects(&wait_handles, None) {
            Some(i) => i,
            _ => continue,
        };

        let mut resize = None;
        keys.clear();
        let mut message = &[][..];

        match wait_handle_index {
            0 => match connection.read_async() {
                Ok(bytes) => message = bytes,
                Err(()) => break,
            },
            1 => {
                let events = read_console_input(input_handle, &mut console_event_buf);
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
                                _ => {
                                    match std::char::decode_utf16(std::iter::once(unicode_char))
                                        .next()
                                    {
                                        Some(Ok(c)) if c.is_ascii_graphic() => Key::Char(c),
                                        _ => continue,
                                    }
                                }
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
        if !connection.write(bytes) {
            break;
        }
    }
}
