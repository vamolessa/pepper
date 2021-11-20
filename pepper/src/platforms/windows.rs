use std::{
    env, io,
    os::windows::{ffi::OsStrExt, io::IntoRawHandle},
    process::Child,
    ptr::NonNull,
    sync::atomic::{AtomicPtr, Ordering},
    time::Duration,
};

use winapi::{
    ctypes::c_void,
    shared::{
        minwindef::{BOOL, DWORD, FALSE, MAX_PATH, TRUE},
        ntdef::NULL,
        winerror::{ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_CONNECTED, WAIT_TIMEOUT},
    },
    um::{
        consoleapi::{GetConsoleMode, ReadConsoleInputW, SetConsoleCtrlHandler, SetConsoleMode},
        debugapi::{DebugBreak, IsDebuggerPresent},
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
        processenv::{GetCommandLineW, GetStdHandle},
        processthreadsapi::{
            CreateProcessW, GetCurrentProcessId, PROCESS_INFORMATION, STARTUPINFOW,
        },
        stringapiset::{MultiByteToWideChar, WideCharToMultiByte},
        synchapi::{CreateEventW, SetEvent, Sleep, WaitForMultipleObjects},
        sysinfoapi::GetSystemDirectoryW,
        winbase::{
            GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, FILE_FLAG_OVERLAPPED,
            FILE_TYPE_CHAR, GMEM_MOVEABLE, INFINITE, NORMAL_PRIORITY_CLASS, PIPE_ACCESS_DUPLEX,
            PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, STARTF_USESTDHANDLES,
            STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, WAIT_OBJECT_0,
        },
        wincon::{
            GetConsoleScreenBufferInfo, CTRL_C_EVENT, ENABLE_PROCESSED_OUTPUT,
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
        },
        wincontypes::{
            INPUT_RECORD, KEY_EVENT, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED,
            RIGHT_CTRL_PRESSED, WINDOW_BUFFER_SIZE_EVENT,
        },
        winnls::CP_UTF8,
        winnt::{
            FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE, HANDLE,
            MAXIMUM_WAIT_OBJECTS,
        },
        winuser::{
            CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
            CF_UNICODETEXT, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME,
            VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
        },
    },
};

use crate::{
    application::{
        ApplicationConfig, ClientApplication, ServerApplication, CLIENT_CONNECTION_BUFFER_LEN,
        CLIENT_STDIN_BUFFER_LEN, SERVER_CONNECTION_BUFFER_LEN, SERVER_IDLE_DURATION,
    },
    client::ClientHandle,
    editor_utils::hash_bytes,
    platform::{
        BufPool, Key, PlatformEvent, PlatformProcessHandle, PlatformRequest, PooledBuf, ProcessTag,
    },
    Args,
};

const MAX_CLIENT_COUNT: usize = 20;
const MAX_PROCESS_COUNT: usize = 43;
const MAX_EVENT_COUNT: usize = 1 + MAX_CLIENT_COUNT + MAX_PROCESS_COUNT;
const _ASSERT_MAX_EVENT_COUNT_IS_MAX_WAIT_OBJECTS: [(); MAXIMUM_WAIT_OBJECTS as _] =
    [(); MAX_EVENT_COUNT];

const CLIENT_EVENT_BUFFER_LEN: usize = 32;
static PIPE_PREFIX: &str = r#"\\.\pipe\"#;

pub fn try_launching_debugger() {
    let mut buf = [0; MAX_PATH + 1];
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as _) as usize };
    if len == 0 || len > buf.len() {
        return;
    }

    let debugger_command = b"\\vsjitdebugger.exe -p ".map(|b| b as u16);
    let mut pid_buf = [0; 10];

    if len + debugger_command.len() + pid_buf.len() + 1 > buf.len() {
        return;
    }

    buf[len..len + debugger_command.len()].copy_from_slice(&debugger_command);
    let len = len + debugger_command.len();

    let pid = unsafe { GetCurrentProcessId() };

    use io::Write;
    let mut pid_cursor = io::Cursor::new(&mut pid_buf[..]);
    let _ = write!(pid_cursor, "{}", pid);
    let pid_len = pid_cursor.position() as usize;
    let pid_buf = pid_buf.map(|b| b as u16);
    let pid_buf = &pid_buf[..pid_len];

    buf[len..len + pid_buf.len()].copy_from_slice(&pid_buf);
    let len = len + pid_buf.len();

    buf[len] = 0;
    let len = len + 1;

    let mut startup_info = unsafe { std::mem::zeroed::<STARTUPINFOW>() };
    startup_info.cb = std::mem::size_of::<STARTUPINFOW>() as _;

    let mut process_info = unsafe { std::mem::zeroed::<PROCESS_INFORMATION>() };

    let result = unsafe {
        CreateProcessW(
            std::ptr::null(),
            buf[..len].as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            FALSE,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut startup_info,
            &mut process_info,
        )
    };

    let _ = Handle(process_info.hProcess);
    let _ = Handle(process_info.hThread);

    if result == FALSE {
        return;
    }

    while unsafe { IsDebuggerPresent() == FALSE } {
        unsafe { Sleep(100) };
    }

    unsafe { DebugBreak() };
}

pub fn main(config: ApplicationConfig) {
    let mut pipe_path = Vec::new();
    let mut hash_buf = [0u8; 16];
    let session_name = match &config.args.session {
        Some(name) => name.as_str(),
        None => {
            use io::Write;

            let current_dir = env::current_dir().expect("could not retrieve the current directory");
            let current_dir_bytes: Vec<_> = current_dir
                .as_os_str()
                .encode_wide()
                .map(|s| {
                    let bytes = s.to_le_bytes();
                    std::iter::once(bytes[0]).chain(std::iter::once(bytes[1]))
                })
                .flatten()
                .collect();

            let current_directory_hash = hash_bytes(&current_dir_bytes);
            let mut cursor = io::Cursor::new(&mut hash_buf[..]);
            write!(&mut cursor, "{:x}", current_directory_hash).unwrap();
            let len = cursor.position() as usize;
            std::str::from_utf8(&hash_buf[..len]).unwrap()
        }
    };

    pipe_path.clear();
    pipe_path.extend(PIPE_PREFIX.encode_utf16());
    pipe_path.extend(session_name.encode_utf16());
    pipe_path.push(0);

    if config.args.print_session {
        print!("{}{}", PIPE_PREFIX, session_name);
        return;
    }

    if config.args.server {
        if !pipe_exists(&pipe_path) {
            let _ = run_server(config, &pipe_path);
        }
    } else {
        if !pipe_exists(&pipe_path) {
            fork();
            while !pipe_exists(&pipe_path) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        run_client(config.args, &pipe_path);
    }
}

fn get_last_error() -> DWORD {
    unsafe { GetLastError() }
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

fn get_std_handle(which: DWORD) -> Option<Handle> {
    let handle = unsafe { GetStdHandle(which) };
    if handle != NULL && handle != INVALID_HANDLE_VALUE {
        Some(Handle(handle))
    } else {
        None
    }
}

fn get_console_size(output_handle: &Handle) -> (usize, usize) {
    let mut console_info = unsafe { std::mem::zeroed() };
    let result = unsafe { GetConsoleScreenBufferInfo(output_handle.0, &mut console_info) };
    if result == FALSE {
        panic!("could not get console size");
    }
    (console_info.dwSize.X as _, console_info.dwSize.Y as _)
}

fn read_console_input<'a>(
    input_handle: &Handle,
    events: &'a mut [INPUT_RECORD],
) -> &'a [INPUT_RECORD] {
    let mut event_count = 0;
    let result = unsafe {
        ReadConsoleInputW(
            input_handle.0,
            events.as_mut_ptr(),
            events.len() as _,
            &mut event_count,
        )
    };
    if result == FALSE {
        panic!(
            "could not read console events {}",
            io::Error::last_os_error()
        );
    }
    &events[..(event_count as usize)]
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
            self.pending_io = false;

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

fn is_pipped(handle: &Handle) -> bool {
    let result = unsafe { GetFileType(handle.0) };
    result != FILE_TYPE_CHAR
}

fn create_file(path: &[u16], share_mode: DWORD, flags: DWORD) -> Option<Handle> {
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            share_mode,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            flags,
            NULL,
        )
    };
    match handle {
        NULL | INVALID_HANDLE_VALUE => None,
        _ => Some(Handle(handle)),
    }
}

enum ReadResult {
    Waiting,
    Ok(usize),
    Err,
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
    startup_info.hStdError = unsafe { GetStdHandle(STD_ERROR_HANDLE) };

    let mut process_info = unsafe { std::mem::zeroed::<PROCESS_INFORMATION>() };

    let mut client_command_line = unsafe { GetCommandLineW() };
    let mut command_line = Vec::new();
    loop {
        unsafe {
            let short = std::ptr::read(client_command_line);
            if short == 0 {
                break;
            }
            client_command_line = client_command_line.offset(1);
            command_line.push(short);
        }
    }
    command_line.extend_from_slice(&b" --server".map(|b| b as _));
    command_line.push(0);

    let result = unsafe {
        CreateProcessW(
            std::ptr::null(),
            command_line.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            TRUE,
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

fn read_from_clipboard(text: &mut String) {
    let clipboard = Clipboard::open();
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle == NULL {
        return;
    }
    let data = match global_lock::<u16>(handle) {
        Some(data) => data,
        None => return,
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

fn set_event(handle: HANDLE) -> bool {
    unsafe { SetEvent(handle) != FALSE }
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
        if !set_event(self.0) {
            panic!("could not notify event");
        }
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

static CTRLC_EVENT_HANDLE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
struct CtrlCEvent(Event);
impl CtrlCEvent {
    pub fn set_ctrl_handler() {
        unsafe extern "system" fn ctrl_handler(ctrl_type: DWORD) -> BOOL {
            match ctrl_type {
                CTRL_C_EVENT => {
                    let handle = CTRLC_EVENT_HANDLE.load(Ordering::Relaxed);
                    if !handle.is_null() {
                        set_event(handle);
                    }
                    TRUE
                }
                _ => FALSE,
            }
        }
        unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) };
    }

    pub fn new() -> Self {
        let event = Event::automatic();
        CTRLC_EVENT_HANDLE.store(event.handle(), Ordering::Relaxed);
        Self(event)
    }

    pub fn event(&self) -> &Event {
        &self.0
    }
}
impl Drop for CtrlCEvent {
    fn drop(&mut self) {
        CTRLC_EVENT_HANDLE.store(std::ptr::null_mut(), Ordering::Relaxed);
    }
}

struct Clipboard;
impl Clipboard {
    pub fn open() -> Self {
        let result = unsafe { OpenClipboard(std::ptr::null_mut()) };
        if result == FALSE {
            panic!("could not open clipboard");
        }
        Self
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
    pub fn new(console_handle: &Handle) -> Self {
        let console_handle = console_handle.0;
        let mut original_mode = 0;
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
            panic!("could not set console mode {}", io::Error::last_os_error());
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
    current_buf: Option<PooledBuf>,
}
impl ConnectionToClient {
    pub fn new(reader: AsyncReader) -> Self {
        Self {
            reader,
            current_buf: None,
        }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn read_async(
        &mut self,
        buf_len: usize,
        buf_pool: &mut BufPool,
    ) -> Result<Option<PooledBuf>, ()> {
        let mut buf = match self.current_buf.take() {
            Some(buf) => buf,
            None => buf_pool.acquire(),
        };
        let write = buf.write_with_len(buf_len);

        match self.reader.read_async(write) {
            ReadResult::Waiting => {
                self.current_buf = Some(buf);
                Ok(None)
            }
            ReadResult::Ok(len) => {
                write.truncate(len);
                Ok(Some(buf))
            }
            ReadResult::Err => {
                buf_pool.release(buf);
                Err(())
            }
        }
    }

    pub fn write(&self, buf: &[u8]) -> bool {
        write_all_bytes(self.reader.handle(), buf)
    }

    pub fn dispose(&mut self, buf_pool: &mut BufPool) {
        if let Some(buf) = self.current_buf.take() {
            buf_pool.release(buf);
        }
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

struct ProcessPipe {
    reader: AsyncReader,
    buf_len: usize,
    current_buf: Option<PooledBuf>,
}
impl ProcessPipe {
    pub fn new(reader: AsyncReader, buf_len: usize) -> Self {
        reader.event.notify();

        Self {
            reader,
            buf_len,
            current_buf: None,
        }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn read_async(&mut self, buf_pool: &mut BufPool) -> Result<Option<PooledBuf>, ()> {
        let mut buf = match self.current_buf.take() {
            Some(buf) => buf,
            None => buf_pool.acquire(),
        };
        let write = buf.write_with_len(self.buf_len);

        match self.reader.read_async(write) {
            ReadResult::Waiting => {
                self.current_buf = Some(buf);
                Ok(None)
            }
            ReadResult::Ok(0) | ReadResult::Err => {
                buf_pool.release(buf);
                Err(())
            }
            ReadResult::Ok(len) => {
                write.truncate(len);
                Ok(Some(buf))
            }
        }
    }
}

struct AsyncProcess {
    alive: bool,
    child: Child,
    tag: ProcessTag,
    pub stdout: Option<ProcessPipe>,
}
impl AsyncProcess {
    pub fn new(mut child: Child, tag: ProcessTag, buf_len: usize) -> Self {
        let stdout = child
            .stdout
            .take()
            .map(IntoRawHandle::into_raw_handle)
            .map(|h| {
                let reader = AsyncReader::new(Handle(h as _));
                ProcessPipe::new(reader, buf_len)
            });

        Self {
            alive: true,
            child,
            tag,
            stdout,
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> bool {
        use io::Write;
        match &mut self.child.stdin {
            Some(stdin) => stdin.write_all(buf).is_ok(),
            None => true,
        }
    }

    pub fn close_input(&mut self) {
        self.child.stdin = None;
    }

    pub fn dispose(&mut self, buf_pool: &mut BufPool) {
        if let Some(buf) = self.stdout.take().and_then(|p| p.current_buf) {
            buf_pool.release(buf);
        }
    }

    pub fn kill(&mut self) {
        if !self.alive {
            return;
        }

        self.alive = false;
        self.stdout = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for AsyncProcess {
    fn drop(&mut self) {
        self.kill();
        self.alive = false;
    }
}

enum EventSource {
    ConnectionListener,
    Connection(usize),
    Process(usize),
}
struct EventListener {
    wait_handles: [HANDLE; MAX_EVENT_COUNT],
    sources: [EventSource; MAX_EVENT_COUNT],
    len: usize,
}
impl EventListener {
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
        debug_assert!(index < self.wait_handles.len());
        self.wait_handles[index] = event.handle();
        self.sources[index] = source;
        self.len += 1;
    }

    pub fn wait_next(&mut self, timeout: Option<Duration>) -> Option<EventSource> {
        let len = self.len;
        self.len = 0;
        let index = wait_for_multiple_objects(&self.wait_handles[..len], timeout)?;
        let mut source = EventSource::ConnectionListener;
        std::mem::swap(&mut source, &mut self.sources[index]);
        Some(source)
    }
}

fn run_server(config: ApplicationConfig, pipe_path: &[u16]) {
    let mut event_listener = EventListener::new();
    let mut listener = ConnectionToClientListener::new(pipe_path, SERVER_CONNECTION_BUFFER_LEN);

    let mut application = match ServerApplication::new(config) {
        Some(application) => application,
        None => return,
    };

    application
        .ctx
        .platform
        .set_clipboard_api(read_from_clipboard, write_to_clipboard);

    let mut client_connections: [Option<ConnectionToClient>; MAX_CLIENT_COUNT] = Default::default();

    const NONE_ASYNC_PROCESS: Option<AsyncProcess> = None;
    let mut processes = [NONE_ASYNC_PROCESS; MAX_PROCESS_COUNT];

    let mut events = Vec::new();
    let mut timeout = None;

    loop {
        event_listener.track(listener.event(), EventSource::ConnectionListener);
        for (i, connection) in client_connections.iter().enumerate() {
            if let Some(connection) = connection {
                event_listener.track(connection.event(), EventSource::Connection(i));
            }
        }
        for (i, process) in processes.iter().enumerate() {
            if let Some(process) = process {
                if let Some(stdout) = &process.stdout {
                    event_listener.track(stdout.event(), EventSource::Process(i));
                }
            }
        }

        let event = match event_listener.wait_next(timeout) {
            Some(event) => {
                timeout = Some(Duration::ZERO);
                event
            }
            None => {
                match timeout {
                    Some(Duration::ZERO) => timeout = Some(SERVER_IDLE_DURATION),
                    Some(_) => {
                        events.push(PlatformEvent::Idle);
                        timeout = None;
                    }
                    None => unreachable!(),
                }

                application.update(events.drain(..));
                let mut requests = application.ctx.platform.requests.drain();
                while let Some(request) = requests.next() {
                    match request {
                        PlatformRequest::Quit => {
                            for connection in client_connections.iter_mut().flatten() {
                                connection.dispose(&mut application.ctx.platform.buf_pool);
                            }
                            for process in processes.iter_mut().flatten() {
                                process.dispose(&mut application.ctx.platform.buf_pool);
                                process.kill();
                            }
                            for request in requests {
                                if let PlatformRequest::WriteToClient { buf, .. }
                                | PlatformRequest::WriteToProcess { buf, .. } = request
                                {
                                    application.ctx.platform.buf_pool.release(buf);
                                }
                            }
                            return;
                        }
                        PlatformRequest::Redraw => timeout = Some(Duration::ZERO),
                        PlatformRequest::WriteToClient { handle, buf } => {
                            if let Some(connection) = &mut client_connections[handle.into_index()] {
                                if !connection.write(buf.as_bytes()) {
                                    connection.dispose(&mut application.ctx.platform.buf_pool);
                                    client_connections[handle.into_index()] = None;
                                    events.push(PlatformEvent::ConnectionClose { handle });
                                }
                            }
                            application.ctx.platform.buf_pool.release(buf);
                        }
                        PlatformRequest::CloseClient { handle } => {
                            if let Some(mut connection) =
                                client_connections[handle.into_index()].take()
                            {
                                connection.dispose(&mut application.ctx.platform.buf_pool)
                            }
                            events.push(PlatformEvent::ConnectionClose { handle });
                        }
                        PlatformRequest::SpawnProcess {
                            tag,
                            mut command,
                            buf_len,
                        } => {
                            let mut spawned = false;
                            for (i, p) in processes.iter_mut().enumerate() {
                                if p.is_some() {
                                    continue;
                                }

                                let handle = PlatformProcessHandle(i as _);
                                if let Ok(child) = command.spawn() {
                                    *p = Some(AsyncProcess::new(child, tag, buf_len));
                                    events.push(PlatformEvent::ProcessSpawned { tag, handle });
                                    spawned = true;
                                }
                                break;
                            }
                            if !spawned {
                                events.push(PlatformEvent::ProcessExit { tag });
                            }
                        }
                        PlatformRequest::WriteToProcess { handle, buf } => {
                            if let Some(process) = &mut processes[handle.0 as usize] {
                                if !process.write(buf.as_bytes()) {
                                    let tag = process.tag;
                                    process.dispose(&mut application.ctx.platform.buf_pool);
                                    process.kill();
                                    processes[handle.0 as usize] = None;
                                    events.push(PlatformEvent::ProcessExit { tag });
                                }
                            }
                            application.ctx.platform.buf_pool.release(buf);
                        }
                        PlatformRequest::CloseProcessInput { handle } => {
                            if let Some(process) = &mut processes[handle.0 as usize] {
                                process.close_input();
                            }
                        }
                        PlatformRequest::KillProcess { handle } => {
                            if let Some(process) = &mut processes[handle.0 as usize] {
                                let tag = process.tag;
                                process.dispose(&mut application.ctx.platform.buf_pool);
                                process.kill();
                                processes[handle.0 as usize] = None;
                                events.push(PlatformEvent::ProcessExit { tag });
                            }
                        }
                    }
                }

                if !events.is_empty() {
                    timeout = Some(Duration::ZERO);
                }

                continue;
            }
        };

        match event {
            EventSource::ConnectionListener => {
                if let Some(connection) = listener.accept(pipe_path) {
                    for (i, c) in client_connections.iter_mut().enumerate() {
                        if c.is_none() {
                            *c = Some(connection);
                            let handle = ClientHandle::from_index(i).unwrap();
                            events.push(PlatformEvent::ConnectionOpen { handle });
                            break;
                        }
                    }
                }
            }
            EventSource::Connection(i) => {
                if let Some(connection) = &mut client_connections[i] {
                    let handle = ClientHandle::from_index(i).unwrap();
                    match connection.read_async(
                        SERVER_CONNECTION_BUFFER_LEN,
                        &mut application.ctx.platform.buf_pool,
                    ) {
                        Ok(None) => (),
                        Ok(Some(buf)) => {
                            events.push(PlatformEvent::ConnectionOutput { handle, buf });
                        }
                        Err(()) => {
                            client_connections[i] = None;
                            events.push(PlatformEvent::ConnectionClose { handle });
                        }
                    }
                }
            }
            EventSource::Process(i) => {
                if let Some(process) = &mut processes[i] {
                    if let Some(pipe) = &mut process.stdout {
                        let tag = process.tag;
                        match pipe.read_async(&mut application.ctx.platform.buf_pool) {
                            Ok(None) => (),
                            Ok(Some(buf)) => events.push(PlatformEvent::ProcessOutput { tag, buf }),
                            Err(()) => {
                                process.stdout = None;
                                process.kill();
                                processes[i] = None;
                                events.push(PlatformEvent::ProcessExit { tag });
                            }
                        }
                    }
                }
            }
        }
    }
}

struct StdinPipe {
    handle: Handle,
    buf: Box<[u8; CLIENT_STDIN_BUFFER_LEN]>,
}
impl StdinPipe {
    pub fn new(handle: Handle) -> Option<Self> {
        if !is_pipped(&handle) {
            return None;
        }

        Some(Self {
            handle,
            buf: Box::new([0; CLIENT_STDIN_BUFFER_LEN]),
        })
    }

    pub fn wait_handle(&self) -> HANDLE {
        self.handle.0
    }

    pub fn read(&mut self) -> Option<&[u8]> {
        let mut read_len = 0;
        let result = unsafe {
            ReadFile(
                self.handle.0,
                self.buf.as_mut_ptr() as _,
                self.buf.len() as _,
                &mut read_len,
                std::ptr::null_mut(),
            )
        };

        if result == FALSE {
            match get_last_error() {
                ERROR_IO_PENDING => None,
                _ => Some(&[]),
            }
        } else {
            Some(&self.buf[..read_len as usize])
        }
    }
}

struct ConnectionToServer {
    reader: AsyncReader,
    buf: Box<[u8; CLIENT_CONNECTION_BUFFER_LEN]>,
}
impl ConnectionToServer {
    pub fn connect(path: &[u16]) -> Self {
        let handle = match create_file(path, 0, FILE_FLAG_OVERLAPPED) {
            Some(handle) => handle,
            None => panic!("could not establish a connection {}", get_last_error()),
        };

        let mut mode = PIPE_READMODE_BYTE;
        let result = unsafe {
            SetNamedPipeHandleState(
                handle.0,
                &mut mode,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };

        if result == FALSE {
            panic!("could not establish a connection");
        }

        let reader = AsyncReader::new(handle);
        let buf = Box::new([0; CLIENT_CONNECTION_BUFFER_LEN]);

        Self { reader, buf }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn write(&mut self, buf: &[u8]) -> bool {
        write_all_bytes(self.reader.handle(), buf)
    }

    pub fn read_async(&mut self) -> Result<&[u8], ()> {
        match self.reader.read_async(&mut self.buf[..]) {
            ReadResult::Waiting => Ok(&[]),
            ReadResult::Ok(0) | ReadResult::Err => Err(()),
            ReadResult::Ok(len) => Ok(&self.buf[..len]),
        }
    }
}

struct ClientOutput(HANDLE);
impl io::Write for ClientOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut write_len = 0;
        let result = unsafe {
            WriteFile(
                self.0,
                buf.as_ptr() as _,
                buf.len() as _,
                &mut write_len,
                std::ptr::null_mut(),
            )
        };
        if result == FALSE {
            return Err(io::Error::last_os_error());
        }

        Ok(write_len as _)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn run_client(args: Args, pipe_path: &[u16]) {
    CtrlCEvent::set_ctrl_handler();

    let mut connection = ConnectionToServer::connect(pipe_path);

    let console_input_handle;
    let console_output_handle;

    if args.quit {
        console_input_handle = None;
        console_output_handle = None;
    } else {
        console_input_handle = {
            let path = b"CONIN$\0".map(|b| b as _);
            let handle = create_file(&path, FILE_SHARE_READ, 0);
            if handle.is_none() {
                panic!("could not open console input");
            }
            handle
        };
        console_output_handle = {
            let path = b"CONOUT$\0".map(|b| b as _);
            let handle = create_file(&path, FILE_SHARE_WRITE, 0);
            if handle.is_none() {
                panic!("could not open console output");
            }
            handle
        };
    };

    let console_input_mode = console_input_handle.as_ref().map(|h| {
        let mode = ConsoleMode::new(h);
        mode.set(ENABLE_WINDOW_INPUT);
        mode
    });
    let console_output_mode = console_output_handle.as_ref().map(|h| {
        let mode = ConsoleMode::new(h);
        mode.set(ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        mode
    });

    let mut application = ClientApplication::new();
    application.output = console_output_handle.as_ref().map(|h| ClientOutput(h.0));

    let bytes = application.init(args);
    if !connection.write(bytes) {
        return;
    }

    if let Some(handle) = &console_output_handle {
        let size = get_console_size(handle);
        let (_, bytes) = application.update(Some(size), &[Key::None], None, &[]);
        if !connection.write(bytes) {
            return;
        }
    }

    let ctrlc_event = CtrlCEvent::new();

    let mut console_event_buf = [unsafe { std::mem::zeroed() }; CLIENT_EVENT_BUFFER_LEN];
    let mut keys = Vec::with_capacity(CLIENT_EVENT_BUFFER_LEN);

    let mut stdin_pipe = get_std_handle(STD_INPUT_HANDLE).and_then(StdinPipe::new);
    let output_handle = get_std_handle(STD_OUTPUT_HANDLE);
    if let Some(handle) = &output_handle {
        if is_pipped(&handle) {
            let (_, bytes) = application.update(None, &[], Some(&[]), &[]);
            if !connection.write(bytes) {
                return;
            }
        }
    }

    let mut wait_handles = [NULL; 4];
    let mut wait_source_map = [0; 4];
    let mut wait_handles_len = 0;

    if let Some(handle) = &console_input_handle {
        wait_handles[wait_handles_len] = handle.0;
        wait_source_map[wait_handles_len] = 0;
        wait_handles_len += 1;
    }

    wait_handles[wait_handles_len] = ctrlc_event.event().handle();
    wait_source_map[wait_handles_len] = 1;
    wait_handles_len += 1;

    wait_handles[wait_handles_len] = connection.event().handle();
    wait_source_map[wait_handles_len] = 2;
    wait_handles_len += 1;

    if let Some(pipe) = &stdin_pipe {
        wait_handles[wait_handles_len] = pipe.wait_handle();
        wait_source_map[wait_handles_len] = 3;
        wait_handles_len += 1;
    }

    let mut wait_handles = &wait_handles[..wait_handles_len];

    loop {
        let wait_source = match wait_for_multiple_objects(wait_handles, None) {
            Some(i) => wait_source_map[i],
            _ => continue,
        };

        let mut resize = None;
        let mut stdin_bytes = None;
        let mut server_bytes = &[][..];

        keys.clear();

        match wait_source {
            0 => {
                if let Some(handle) = &console_input_handle {
                    let console_events = read_console_input(handle, &mut console_event_buf);
                    parse_console_events(console_events, &mut keys, &mut resize);
                }
            }
            1 => keys.push(Key::Ctrl('c')),
            2 => match connection.read_async() {
                Ok(bytes) => server_bytes = bytes,
                Err(()) => break,
            },
            3 => stdin_bytes = stdin_pipe.as_mut().and_then(StdinPipe::read),
            _ => unreachable!(),
        }

        let (_, bytes) = application.update(resize, &keys, stdin_bytes, server_bytes);
        if !connection.write(bytes) {
            break;
        }
        if let Some(&[]) = stdin_bytes {
            stdin_pipe = None;
            wait_handles = &wait_handles[..wait_handles.len() - 1];
        }
    }

    if let Some(handle) = output_handle {
        if is_pipped(&handle) {
            let bytes = application.get_stdout_bytes();
            write_all_bytes(&handle, bytes);
        }
    }

    drop(console_input_mode);
    drop(console_output_mode);

    drop(application);
    drop(console_input_handle);
    drop(console_output_handle);
}

fn parse_console_events(
    console_events: &[INPUT_RECORD],
    keys: &mut Vec<Key>,
    resize: &mut Option<(usize, usize)>,
) {
    for event in console_events {
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
                        match std::char::decode_utf16(std::iter::once(unicode_char)).next() {
                            Some(Ok(c)) => Key::Char(c),
                            _ => continue,
                        }
                    }
                    CHAR_A..=CHAR_Z => {
                        const ALT_PRESSED_MASK: DWORD = LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED;
                        const CTRL_PRESSED_MASK: DWORD = LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED;

                        if control_key_state & ALT_PRESSED_MASK != 0 {
                            let c = (keycode - CHAR_A) as u8 + b'a';
                            Key::Alt(c.to_ascii_lowercase() as _)
                        } else if control_key_state & CTRL_PRESSED_MASK != 0 {
                            let c = (keycode - CHAR_A) as u8 + b'a';
                            Key::Ctrl(c.to_ascii_lowercase() as _)
                        } else {
                            match std::char::decode_utf16(std::iter::once(unicode_char)).next() {
                                Some(Ok(c)) => Key::Char(c),
                                _ => continue,
                            }
                        }
                    }
                    _ => match std::char::decode_utf16(std::iter::once(unicode_char)).next() {
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
                *resize = Some((size.X as _, size.Y as _));
            }
            _ => (),
        }
    }
}
