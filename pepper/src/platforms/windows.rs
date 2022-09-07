use std::{
    collections::VecDeque,
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
        minwindef::{BOOL, DWORD, FALSE, TRUE},
        ntdef::NULL,
        winerror::{ERROR_IO_PENDING, ERROR_PIPE_LISTENING, ERROR_BROKEN_PIPE, ERROR_MORE_DATA, ERROR_PIPE_CONNECTED, WAIT_TIMEOUT},
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
        synchapi::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject},
        winbase::{
            GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, CREATE_NEW_PROCESS_GROUP,
            CREATE_NO_WINDOW, FILE_FLAG_OVERLAPPED, FILE_TYPE_CHAR, GMEM_MOVEABLE, INFINITE,
            NORMAL_PRIORITY_CLASS, PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE, PIPE_READMODE_MESSAGE,
            PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, STARTF_USESTDHANDLES, STD_ERROR_HANDLE,
            STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, WAIT_OBJECT_0,
        },
        wincon::{
            GetConsoleScreenBufferInfo, CTRL_C_EVENT, ENABLE_PROCESSED_OUTPUT,
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
        },
        wincontypes::{
            INPUT_RECORD, KEY_EVENT, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED,
            RIGHT_CTRL_PRESSED, SHIFT_PRESSED, WINDOW_BUFFER_SIZE_EVENT,
        },
        winnls::CP_UTF8,
        winnt::{
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, GENERIC_READ, GENERIC_WRITE,
            HANDLE, MAXIMUM_WAIT_OBJECTS,
        },
        winuser::{
            CloseClipboard, EmptyClipboard, GetClipboardData, MessageBoxW, OpenClipboard,
            SetClipboardData, CF_UNICODETEXT, IDYES, MB_ICONEXCLAMATION, MB_YESNO, VK_BACK,
            VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_LEFT, VK_NEXT,
            VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
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
        drop_request, BufPool, IpcReadMode, IpcTag, Key, KeyCode, PlatformEvent, PlatformIpcHandle,
        PlatformProcessHandle, PlatformRequest, PooledBuf, ProcessTag,
    },
    Args,
};

const MAX_EVENT_COUNT: usize = MAXIMUM_WAIT_OBJECTS as _;
const EVENT_COUNT_PER_CLIENT: usize = 2;
const EVENT_COUNT_PER_PROCESS: usize = 1;

const CLIENT_EVENT_BUFFER_LEN: usize = 32;

#[inline(always)]
pub fn try_attach_debugger() {
    fn ask_for_debugging() -> bool {
        let text = b"the editor crashed!\nwant to debug it?\0".map(|b| b as u16);
        let caption = b"debug crash?\0".map(|b| b as u16);
        let message_box_id = unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                &text as _,
                &caption as _,
                MB_ICONEXCLAMATION | MB_YESNO,
            )
        };
        message_box_id == IDYES
    }

    fn try_spawn_debugger(command_ascii: &[u8]) -> bool {
        use io::Write;

        let pid = unsafe { GetCurrentProcessId() };
        let mut pid_buf = [0; 16];
        let mut pid_cursor = io::Cursor::new(&mut pid_buf[..]);
        if write!(pid_cursor, " {}", pid).is_err() {
            return false;
        }
        let pid_len = pid_cursor.position() as usize;
        let len = command_ascii.len() + pid_len + 1;

        let mut buf = [0; 512];
        if buf.len() < len {
            return false;
        }

        let pid_bytes = &pid_buf[..pid_len];
        for (i, &b) in command_ascii.iter().chain(pid_bytes.iter()).enumerate() {
            buf[i] = b as _;
        }
        let command = &mut buf[..len];

        let mut startup_info = unsafe { std::mem::zeroed::<STARTUPINFOW>() };
        startup_info.cb = std::mem::size_of::<STARTUPINFOW>() as _;
        let mut process_info = unsafe { std::mem::zeroed::<PROCESS_INFORMATION>() };

        let result = unsafe {
            CreateProcessW(
                std::ptr::null(),
                command.as_mut_ptr(),
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
        if result == FALSE {
            return false;
        }

        let process_handle = Handle(process_info.hProcess);
        let thread_handle = Handle(process_info.hThread);

        unsafe { WaitForSingleObject(process_handle.0, INFINITE) };

        drop(process_handle);
        drop(thread_handle);

        true
    }

    if unsafe { IsDebuggerPresent() == FALSE } {
        if !ask_for_debugging() {
            return;
        }

        let debuggers = [
            "remedybg.exe attach-to-process-by-id",
            "vsjitdebugger.exe -p",
        ];
        for debugger in debuggers {
            if try_spawn_debugger(debugger.as_bytes()) {
                break;
            }
        }
    }

    unsafe { DebugBreak() };
}

const PIPE_PREFIX: &str = r#"\\.\pipe\"#;

pub fn main(mut config: ApplicationConfig) {
    if config.args.session_name.is_empty() {
        use std::fmt::Write;

        let current_dir = env::current_dir().expect("could not retrieve the current directory");
        let current_dir_bytes: Vec<_> = current_dir
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();

        let current_directory_hash = hash_bytes(&current_dir_bytes);
        write!(config.args.session_name, "{:x}", current_directory_hash).unwrap();
    }

    let mut pipe_path = Vec::new();
    pipe_path.extend(PIPE_PREFIX.encode_utf16());
    pipe_path.extend(env!("CARGO_PKG_NAME").encode_utf16());
    pipe_path.push(b'-' as _);
    pipe_path.extend(config.args.session_name.encode_utf16());
    pipe_path.push(b'\0' as _);

    if config.args.print_session {
        print!(
            "{}{}-{}",
            PIPE_PREFIX,
            env!("CARGO_PKG_NAME"),
            &config.args.session_name
        );
        return;
    }

    if config.args.server {
        if !pipe_exists(&pipe_path) {
            run_server(config, &pipe_path);
        }
    } else {
        if !pipe_exists(&pipe_path) {
            spawn_server();
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

fn get_console_size(output_handle: &Handle) -> (u16, u16) {
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

enum IoResult {
    Waiting,
    Ok(usize),
    Err,
}

struct AsyncIO {
    raw_handle: HANDLE,
    event: Event,
    overlapped: Overlapped,
    pending_io: bool,
}
impl AsyncIO {
    pub fn new(raw_handle: HANDLE) -> Self {
        let event = Event::manual(true);
        let overlapped = Overlapped::with_event(&event);
        //let overlapped = Overlapped::default();

        Self {
            raw_handle,
            event,
            overlapped,
            pending_io: false,
        }
    }

    pub fn event(&self) -> &Event {
        &self.event
    }

    pub fn overlapped(&mut self) -> &mut Overlapped {
        &mut self.overlapped
    }

    pub fn read_async_new(&mut self, buf: &mut [u8]) -> IoResult {
        let mut read_len = 0;

        let mut pending_io = self.overlapped.0.hEvent != std::ptr::null_mut();
        if pending_io {
            let result = unsafe {
                GetOverlappedResult(
                    self.raw_handle,
                    self.overlapped.as_mut_ptr(),
                    &mut read_len,
                    FALSE,
                )
            };
            if result == FALSE {
                match get_last_error() {
                    ERROR_BROKEN_PIPE => return IoResult::Err,
                    ERROR_MORE_DATA => (),
                    error => panic!("error on async read: {}", error),
                }
            }
        }

        self.overlapped.0.hEvent = self.event.0;
        let result = unsafe {
            ReadFile(
                self.raw_handle,
                buf.as_mut_ptr() as _,
                buf.len() as _,
                &mut read_len,
                self.overlapped.as_mut_ptr(),
            )
        };
        if result == FALSE {
            match get_last_error() {
                ERROR_IO_PENDING | ERROR_PIPE_LISTENING => pending_io = true,
                error => panic!("error on async read: {}", error),
            }
        }

        if pending_io {
            IoResult::Waiting
        } else {
            self.event.notify();
            IoResult::Ok(read_len as _)
        }
    }

    pub fn read_async(&mut self, buf: &mut [u8]) -> IoResult {
        let mut read_len = 0;
        if self.pending_io {
            self.pending_io = false;

            let result = unsafe {
                GetOverlappedResult(
                    self.raw_handle,
                    self.overlapped.as_mut_ptr(),
                    &mut read_len,
                    FALSE,
                )
            };

            if result == FALSE {
                match get_last_error() {
                    ERROR_MORE_DATA => {
                        self.event.notify();
                        IoResult::Ok(read_len as _)
                    }
                    _ => IoResult::Err,
                }
            } else {
                self.event.notify();
                IoResult::Ok(read_len as _)
            }
        } else {
            let result = unsafe {
                ReadFile(
                    self.raw_handle,
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
                        IoResult::Waiting
                    }
                    ERROR_MORE_DATA => {
                        self.event.notify();
                        IoResult::Ok(read_len as _)
                    }
                    _ => IoResult::Err,
                }
            } else {
                self.event.notify();
                IoResult::Ok(read_len as _)
            }
        }
    }

    pub fn write_async_new(&mut self, buf: &[u8]) -> IoResult {
        let mut write_len = 0;

        let mut pending_io = self.overlapped.0.hEvent != std::ptr::null_mut();
        if pending_io {
            let result = unsafe {
                GetOverlappedResult(
                    self.raw_handle,
                    self.overlapped.as_mut_ptr(),
                    &mut write_len,
                    FALSE,
                )
            };
            if result == FALSE {
                match get_last_error() {
                    ERROR_BROKEN_PIPE => return IoResult::Err,
                    ERROR_MORE_DATA => (),
                    error => panic!("error on async write: {}", error),
                }
            }
        }

        self.overlapped.0.hEvent = self.event.0;
        let result = unsafe {
            WriteFile(
                self.raw_handle,
                buf.as_ptr() as _,
                buf.len() as _,
                &mut write_len,
                self.overlapped.as_mut_ptr(),
            )
        };
        if result == FALSE {
            match get_last_error() {
                ERROR_IO_PENDING => pending_io = true,
                error => panic!("error on async write: {}", error),
            }
        }

        if pending_io {
            IoResult::Waiting
        } else {
            self.event.notify();
            IoResult::Ok(write_len as _)
        }
    }

    pub fn write_async(&mut self, buf: &[u8]) -> IoResult {
        let mut write_len = 0;
        if self.pending_io {
            self.pending_io = false;

            let result = unsafe {
                GetOverlappedResult(
                    self.raw_handle,
                    self.overlapped.as_mut_ptr(),
                    &mut write_len,
                    FALSE,
                )
            };

            if result == FALSE {
                match get_last_error() {
                    ERROR_MORE_DATA => {
                        self.event.notify();
                        IoResult::Ok(write_len as _)
                    }
                    _ => IoResult::Err,
                }
            } else {
                self.event.notify();
                IoResult::Ok(write_len as _)
            }
        } else {
            let result = unsafe {
                WriteFile(
                    self.raw_handle,
                    buf.as_ptr() as _,
                    buf.len() as _,
                    &mut write_len,
                    self.overlapped.as_mut_ptr(),
                )
            };

            if result == FALSE {
                match get_last_error() {
                    ERROR_IO_PENDING => {
                        self.pending_io = true;
                        IoResult::Waiting
                    }
                    _ => IoResult::Err,
                }
            } else {
                self.event.notify();
                IoResult::Ok(write_len as _)
            }
        }
    }
}

fn is_pipped(handle: &Handle) -> bool {
    let result = unsafe { GetFileType(handle.0) };
    result != FILE_TYPE_CHAR
}

fn create_file(
    path: &[u16],
    access_mode: DWORD,
    share_mode: DWORD,
    flags: DWORD,
) -> Option<Handle> {
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access_mode,
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

fn spawn_server() {
    let mut startup_info = unsafe { std::mem::zeroed::<STARTUPINFOW>() };
    startup_info.cb = std::mem::size_of::<STARTUPINFOW>() as _;
    startup_info.dwFlags = STARTF_USESTDHANDLES;
    startup_info.hStdInput = INVALID_HANDLE_VALUE;
    startup_info.hStdOutput = INVALID_HANDLE_VALUE;
    startup_info.hStdError = unsafe { GetStdHandle(STD_ERROR_HANDLE) };

    let mut process_info = unsafe { std::mem::zeroed::<PROCESS_INFORMATION>() };

    let client_command_line = unsafe {
        let command_line_start = GetCommandLineW();
        let mut command_line_end = command_line_start;
        loop {
            if std::ptr::read(command_line_end) == 0 {
                break;
            }
            command_line_end = command_line_end.offset(1);
        }
        let len = command_line_end.offset_from(command_line_start);
        std::slice::from_raw_parts(command_line_start, len as _)
    };

    if client_command_line.is_empty() {
        panic!("executable command line was empty");
    }
    let application_name_len = if client_command_line[0] == b'"' as _ {
        match client_command_line[1..]
            .iter()
            .position(|&s| s == b'"' as _)
        {
            Some(i) => i + 2,
            None => client_command_line.len(),
        }
    } else {
        match client_command_line.iter().position(|&s| s == b' ' as _) {
            Some(i) => i,
            None => client_command_line.len(),
        }
    };
    let (client_application_name, client_command_line) =
        client_command_line.split_at(application_name_len);

    let server_flag = b" --server";
    let mut command_line = Vec::with_capacity(client_command_line.len() + server_flag.len());
    command_line.extend_from_slice(client_application_name);
    command_line.extend_from_slice(&server_flag.map(|b| b as _));
    command_line.extend_from_slice(client_command_line);
    command_line.push(0);

    let result = unsafe {
        CreateProcessW(
            std::ptr::null(),
            command_line.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            TRUE,
            NORMAL_PRIORITY_CLASS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
            NULL,
            std::ptr::null_mut(),
            &mut startup_info,
            &mut process_info,
        )
    };

    std::mem::drop(Handle(process_info.hProcess));
    std::mem::drop(Handle(process_info.hThread));

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
    pub fn automatic(initial_state: bool) -> Self {
        Self(create_event(false, initial_state))
    }

    pub fn manual(initial_state: bool) -> Self {
        Self(create_event(true, initial_state))
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
        let result = unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) };
        if result == FALSE {
            panic!("could not set ctrl handler");
        }
    }

    pub fn new() -> Self {
        let event = Event::automatic(false);
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
        let mut overlapped = Self::default();
        overlapped.0.hEvent = event.handle();
        overlapped
    }

    pub fn as_mut_ptr(&mut self) -> *mut OVERLAPPED {
        &mut self.0
    }
}
impl Default for Overlapped {
    fn default() -> Self {
        Self(unsafe { std::mem::zeroed::<OVERLAPPED>() })
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
    handle: Handle,
    reader: AsyncIO,
    read_buf: Option<PooledBuf>,
    writer: AsyncIO,
    write_buf_queue: VecDeque<PooledBuf>,
}
impl ConnectionToClient {
    pub fn new(handle: Handle, reader: AsyncIO) -> Self {
        let raw_handle = handle.0;
        Self {
            handle,
            reader,
            read_buf: None,
            writer: AsyncIO::new(raw_handle),
            write_buf_queue: VecDeque::new(),
        }
    }

    pub fn read_event(&self) -> &Event {
        self.reader.event()
    }

    pub fn write_event(&self) -> Option<&Event> {
        if self.write_buf_queue.is_empty() {
            None
        } else {
            Some(self.writer.event())
        }
    }

    pub fn read_async(
        &mut self,
        buf_len: usize,
        buf_pool: &mut BufPool,
    ) -> Result<Option<PooledBuf>, ()> {
        let mut buf = match self.read_buf.take() {
            Some(buf) => buf,
            None => buf_pool.acquire(),
        };
        let write = buf.write_with_len(buf_len);

        match self.reader.read_async(write) {
            IoResult::Waiting => {
                self.read_buf = Some(buf);
                Ok(None)
            }
            IoResult::Ok(len) => {
                write.truncate(len);
                Ok(Some(buf))
            }
            IoResult::Err => {
                buf_pool.release(buf);
                Err(())
            }
        }
    }

    pub fn enqueue_write(&mut self, buf: PooledBuf) {
        self.write_buf_queue.push_back(buf);
    }

    pub fn write_pending_async(&mut self) -> Result<Option<PooledBuf>, VecDeque<PooledBuf>> {
        match self.write_buf_queue.get_mut(0) {
            Some(buf) => match self.writer.write_async(buf.as_bytes()) {
                IoResult::Waiting => Ok(None),
                IoResult::Ok(len) => {
                    buf.drain_start(len);
                    if buf.as_bytes().is_empty() {
                        Ok(self.write_buf_queue.pop_front())
                    } else {
                        Ok(None)
                    }
                }
                IoResult::Err => {
                    let mut bufs = VecDeque::new();
                    std::mem::swap(&mut bufs, &mut self.write_buf_queue);
                    Err(bufs)
                }
            },
            None => unreachable!(),
        }
    }

    pub fn dispose(&mut self, buf_pool: &mut BufPool) {
        if let Some(buf) = self.read_buf.take() {
            buf_pool.release(buf);
        }
        for buf in self.write_buf_queue.drain(..) {
            buf_pool.release(buf);
        }
    }
}
impl Drop for ConnectionToClient {
    fn drop(&mut self) {
        unsafe { DisconnectNamedPipe(self.handle.0) };
    }
}

struct ConnectionToClientListener {
    handle: Handle,
    reader: AsyncIO,
    buf: Box<[u8]>,
}
impl ConnectionToClientListener {
    fn new_listen_reader(pipe_path: &[u16], buf_len: usize) -> (Handle, AsyncIO) {
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

        let handle = Handle(handle);
        let mut reader = AsyncIO::new(handle.0);

        if unsafe { ConnectNamedPipe(handle.0, reader.overlapped().as_mut_ptr()) } != FALSE {
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

        (handle, reader)
    }

    pub fn new(pipe_path: &[u16], buf_len: usize) -> Self {
        let (handle, reader) = Self::new_listen_reader(pipe_path, buf_len);

        let mut buf = Vec::with_capacity(buf_len);
        buf.resize(buf_len, 0);
        let buf = buf.into_boxed_slice();

        Self {
            handle,
            reader,
            buf,
        }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn accept(&mut self, pipe_path: &[u16]) -> Option<ConnectionToClient> {
        match self.reader.read_async(&mut self.buf) {
            IoResult::Waiting => None,
            IoResult::Ok(_) => {
                let (mut handle, mut reader) = Self::new_listen_reader(pipe_path, self.buf.len());
                std::mem::swap(&mut handle, &mut self.handle);
                std::mem::swap(&mut reader, &mut self.reader);
                Some(ConnectionToClient::new(handle, reader))
            }
            IoResult::Err => panic!("could not accept connection {}", get_last_error()),
        }
    }
}

struct ProcessPipe {
    _handle: Handle,
    reader: AsyncIO,
    buf_len: usize,
    current_buf: Option<PooledBuf>,
}
impl ProcessPipe {
    pub fn new(handle: Handle, buf_len: usize) -> Self {
        let reader = AsyncIO::new(handle.0);
        reader.event.notify();

        Self {
            _handle: handle,
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
            IoResult::Waiting => {
                self.current_buf = Some(buf);
                Ok(None)
            }
            IoResult::Ok(0) | IoResult::Err => {
                buf_pool.release(buf);
                Err(())
            }
            IoResult::Ok(len) => {
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
            .map(|h| ProcessPipe::new(Handle(h as _), buf_len));

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

struct AsyncIpc {
    tag: IpcTag,
    _handle: Handle,
    buf_len: usize,
    read_mode: IpcReadMode,

    reader: Option<AsyncIO>,
    read_buf: Option<PooledBuf>,
    partial_read_buf: Vec<u8>,

    writer: Option<AsyncIO>,
    write_buf_queue: VecDeque<PooledBuf>,
}
impl AsyncIpc {
    pub fn connect(
        tag: IpcTag,
        pipe_path: &[u16],
        read: bool,
        write: bool,
        read_mode: IpcReadMode,
        buf_len: usize,
    ) -> Option<Self> {
        if !read && !write {
            return None;
        }

        let mut access_mode = 0;
        if read {
            access_mode |= GENERIC_READ;
        }
        if write {
            access_mode |= GENERIC_WRITE;
        }
        if read && !write {
            access_mode |= FILE_WRITE_ATTRIBUTES;
        }

        let mut mode = match read_mode {
            IpcReadMode::ByteStream => PIPE_READMODE_BYTE,
            IpcReadMode::MessageStream => PIPE_READMODE_MESSAGE,
        };

        for _ in 0..8 {
            if pipe_exists(&pipe_path) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let handle = create_file(pipe_path, access_mode, 0, FILE_FLAG_OVERLAPPED)?;

        let result = unsafe {
            SetNamedPipeHandleState(
                handle.0,
                &mut mode,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };

        if result == FALSE {
            return None;
        }

        let raw_handle = handle.0;
        Some(Self {
            tag,
            _handle: handle,
            buf_len,
            read_mode,

            reader: read.then(|| AsyncIO::new(raw_handle)),
            read_buf: None,
            partial_read_buf: Vec::new(),

            writer: write.then(|| AsyncIO::new(raw_handle)),
            write_buf_queue: VecDeque::new(),
        })
    }

    pub fn event_count(&self) -> usize {
        self.reader.is_some() as usize + self.writer.is_some() as usize
    }

    pub fn read_event(&self) -> Option<&Event> {
        self.reader.as_ref().map(AsyncIO::event)
    }

    pub fn write_event(&self) -> Option<&Event> {
        if self.write_buf_queue.is_empty() {
            None
        } else {
            self.writer.as_ref().map(AsyncIO::event)
        }
    }

    pub fn read_async(&mut self, buf_pool: &mut BufPool) -> Result<Option<PooledBuf>, ()> {
        match &mut self.reader {
            Some(reader) => {
                let mut buf = match self.read_buf.take() {
                    Some(buf) => buf,
                    None => buf_pool.acquire(),
                };
                let write = buf.write_with_len(self.buf_len);

                match reader.read_async(write) {
                    IoResult::Waiting => {
                        self.read_buf = Some(buf);
                        Ok(None)
                    }
                    IoResult::Ok(len) => {
                        write.truncate(len);
                        match self.read_mode {
                            IpcReadMode::MessageStream if get_last_error() == ERROR_MORE_DATA => {
                                if len == 0 {
                                    //OutputDebugStringA("more data with len == 0");
                                } else {
                                    //OutputDebugStringA("more data with len > 0");
                                }
                                self.partial_read_buf.extend_from_slice(write);
                                self.read_buf = Some(buf);
                                Ok(None)
                            }
                            _ => {
                                if !self.partial_read_buf.is_empty() {
                                    self.partial_read_buf.extend_from_slice(write);
                                    std::mem::swap(&mut self.partial_read_buf, write);
                                    self.partial_read_buf.clear();
                                }
                                Ok(Some(buf))
                            }
                        }
                    }
                    IoResult::Err => {
                        self.partial_read_buf.clear();
                        buf_pool.release(buf);
                        Err(())
                    }
                }
            }
            None => Ok(None),
        }
    }

    pub fn enqueue_write(&mut self, buf: PooledBuf) {
        if self.writer.is_some() {
            self.write_buf_queue.push_back(buf);
        }
    }

    pub fn write_pending_async(&mut self) -> Result<Option<PooledBuf>, VecDeque<PooledBuf>> {
        match &mut self.writer {
            Some(writer) => match self.write_buf_queue.get_mut(0) {
                Some(buf) => match writer.write_async(buf.as_bytes()) {
                    IoResult::Waiting => Ok(None),
                    IoResult::Ok(len) => {
                        buf.drain_start(len);
                        if buf.as_bytes().is_empty() {
                            Ok(self.write_buf_queue.pop_front())
                        } else {
                            Ok(None)
                        }
                    }
                    IoResult::Err => {
                        let mut bufs = VecDeque::new();
                        std::mem::swap(&mut bufs, &mut self.write_buf_queue);
                        Err(bufs)
                    }
                },
                None => unreachable!(),
            },
            None => Ok(None),
        }
    }

    pub fn dispose(&mut self, buf_pool: &mut BufPool) {
        if let Some(buf) = self.read_buf.take() {
            buf_pool.release(buf);
        }
        for buf in self.write_buf_queue.drain(..) {
            buf_pool.release(buf);
        }
    }
}

enum EventSource {
    ConnectionListener,
    ConnectionRead(u8),
    ConnectionWrite(u8),
    Process(u8),
    IpcRead(u8),
    IpcWrite(u8),
}
struct EventListener {
    wait_handles: [HANDLE; MAX_EVENT_COUNT],
    sources: [EventSource; MAX_EVENT_COUNT],
    len: u8,
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
        let index = self.len as usize;
        assert!(index < self.wait_handles.len());

        self.wait_handles[index] = event.handle();
        self.sources[index] = source;
        self.len += 1;
    }

    #[inline(never)]
    pub fn wait_next(&mut self, timeout: Option<Duration>) -> Option<EventSource> {
        let len = self.len;
        self.len = 0;
        let index = wait_for_multiple_objects(&self.wait_handles[..len as usize], timeout)?;
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

    const NONE_CONNECTION_TO_CLIENT: Option<ConnectionToClient> = None;
    let mut client_connections = [NONE_CONNECTION_TO_CLIENT; MAX_EVENT_COUNT];

    const NONE_ASYNC_PROCESS: Option<AsyncProcess> = None;
    let mut processes = [NONE_ASYNC_PROCESS; MAX_EVENT_COUNT];

    const NONE_ASYNC_IPC: Option<AsyncIpc> = None;
    let mut ipcs = [NONE_ASYNC_IPC; MAX_EVENT_COUNT];

    let mut events = Vec::new();
    let mut timeout = None;
    let mut need_redraw = false;

    let mut ipc_path_u16 = Vec::new();

    loop {
        event_listener.track(listener.event(), EventSource::ConnectionListener);
        let mut event_count = 1;
        for (i, connection) in client_connections.iter().enumerate() {
            if let Some(connection) = connection {
                event_count += EVENT_COUNT_PER_CLIENT;
                event_listener.track(connection.read_event(), EventSource::ConnectionRead(i as _));
                if let Some(event) = connection.write_event() {
                    event_listener.track(event, EventSource::ConnectionWrite(i as _));
                }
            }
        }
        for (i, process) in processes.iter().enumerate() {
            if let Some(process) = process {
                event_count += EVENT_COUNT_PER_PROCESS;
                if let Some(stdout) = &process.stdout {
                    event_listener.track(stdout.event(), EventSource::Process(i as _));
                }
            }
        }
        for (i, ipc) in ipcs.iter().enumerate() {
            if let Some(ipc) = ipc {
                event_count += ipc.event_count();
                if let Some(event) = ipc.read_event() {
                    event_listener.track(event, EventSource::IpcRead(i as _));
                }
                if let Some(event) = ipc.write_event() {
                    event_listener.track(event, EventSource::IpcWrite(i as _));
                }
            }
        }

        let previous_timeout = timeout;
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
                    None => continue,
                }

                if events.is_empty() && !need_redraw {
                    continue;
                }

                need_redraw = false;
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
                            for ipc in ipcs.iter_mut().flatten() {
                                ipc.dispose(&mut application.ctx.platform.buf_pool);
                            }
                            for request in requests {
                                drop_request(&mut application.ctx.platform.buf_pool, request);
                            }
                            return;
                        }
                        PlatformRequest::Redraw => {
                            need_redraw = true;
                            timeout = Some(Duration::ZERO);
                        }
                        PlatformRequest::WriteToClient { handle, buf } => {
                            let index = handle.0 as usize;
                            match &mut client_connections[index] {
                                Some(connection) => connection.enqueue_write(buf),
                                None => application.ctx.platform.buf_pool.release(buf),
                            }
                        }
                        PlatformRequest::CloseClient { handle } => {
                            let index = handle.0 as usize;
                            if let Some(mut connection) = client_connections[index].take() {
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
                            if event_count + EVENT_COUNT_PER_PROCESS <= MAX_EVENT_COUNT {
                                for (i, p) in processes.iter_mut().enumerate() {
                                    if p.is_some() {
                                        continue;
                                    }

                                    if let Ok(child) = command.spawn() {
                                        *p = Some(AsyncProcess::new(child, tag, buf_len));
                                        let handle = PlatformProcessHandle(i as _);
                                        events.push(PlatformEvent::ProcessSpawned { tag, handle });
                                        spawned = true;
                                    }
                                    break;
                                }
                            }
                            if !spawned {
                                events.push(PlatformEvent::ProcessExit { tag });
                            }
                        }
                        PlatformRequest::WriteToProcess { handle, buf } => {
                            let index = handle.0 as usize;
                            if let Some(process) = &mut processes[index] {
                                if !process.write(buf.as_bytes()) {
                                    let tag = process.tag;
                                    process.dispose(&mut application.ctx.platform.buf_pool);
                                    process.kill();
                                    processes[index] = None;
                                    events.push(PlatformEvent::ProcessExit { tag });
                                }
                            }
                            application.ctx.platform.buf_pool.release(buf);
                        }
                        PlatformRequest::CloseProcessInput { handle } => {
                            let index = handle.0 as usize;
                            if let Some(process) = &mut processes[index] {
                                process.close_input();
                            }
                        }
                        PlatformRequest::KillProcess { handle } => {
                            let index = handle.0 as usize;
                            if let Some(mut process) = processes[index].take() {
                                let tag = process.tag;
                                process.dispose(&mut application.ctx.platform.buf_pool);
                                process.kill();
                                events.push(PlatformEvent::ProcessExit { tag });
                            }
                        }
                        PlatformRequest::ConnectToIpc {
                            tag,
                            path,
                            read,
                            write,
                            read_mode,
                            buf_len,
                        } => {
                            let ipc_event_count = read as usize + write as usize;
                            let mut connected = false;
                            if event_count + ipc_event_count <= MAX_EVENT_COUNT {
                                for (i, ipc) in ipcs.iter_mut().enumerate() {
                                    if ipc.is_some() {
                                        continue;
                                    }

                                    if let Ok(path) = std::str::from_utf8(path.as_bytes()) {
                                        ipc_path_u16.clear();
                                        ipc_path_u16.extend(PIPE_PREFIX.encode_utf16());
                                        ipc_path_u16.extend(path.encode_utf16());
                                        ipc_path_u16.push(0);
                                        *ipc = AsyncIpc::connect(
                                            tag,
                                            &ipc_path_u16,
                                            read,
                                            write,
                                            read_mode,
                                            buf_len,
                                        );
                                        if ipc.is_some() {
                                            let handle = PlatformIpcHandle(i as _);
                                            events
                                                .push(PlatformEvent::IpcConnected { tag, handle });
                                            connected = true;
                                        }
                                    }
                                    break;
                                }
                            }
                            if !connected {
                                events.push(PlatformEvent::IpcClose { tag });
                            }

                            application.ctx.platform.buf_pool.release(path);
                        }
                        PlatformRequest::WriteToIpc { handle, buf } => {
                            let index = handle.0 as usize;
                            match &mut ipcs[index] {
                                Some(ipc) => ipc.enqueue_write(buf),
                                None => application.ctx.platform.buf_pool.release(buf),
                            }
                        }
                        PlatformRequest::CloseIpc { handle } => {
                            let index = handle.0 as usize;
                            if let Some(mut ipc) = ipcs[index].take() {
                                let tag = ipc.tag;
                                ipc.dispose(&mut application.ctx.platform.buf_pool);
                                events.push(PlatformEvent::IpcClose { tag });
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
                    if event_count + EVENT_COUNT_PER_CLIENT <= MAX_EVENT_COUNT {
                        for (i, c) in client_connections.iter_mut().enumerate() {
                            if c.is_none() {
                                *c = Some(connection);
                                let handle = ClientHandle(i as _);
                                events.push(PlatformEvent::ConnectionOpen { handle });
                                break;
                            }
                        }
                    }
                }
            }
            EventSource::ConnectionRead(i) => {
                if let Some(connection) = &mut client_connections[i as usize] {
                    let handle = ClientHandle(i);
                    match connection.read_async(
                        SERVER_CONNECTION_BUFFER_LEN,
                        &mut application.ctx.platform.buf_pool,
                    ) {
                        Ok(None) => (),
                        Ok(Some(buf)) => {
                            events.push(PlatformEvent::ConnectionOutput { handle, buf });
                        }
                        Err(()) => {
                            connection.dispose(&mut application.ctx.platform.buf_pool);
                            client_connections[i as usize] = None;
                            events.push(PlatformEvent::ConnectionClose { handle });
                        }
                    }
                }
            }
            EventSource::ConnectionWrite(i) => {
                timeout = previous_timeout;

                if let Some(connection) = &mut client_connections[i as usize] {
                    match connection.write_pending_async() {
                        Ok(None) => (),
                        Ok(Some(buf)) => application.ctx.platform.buf_pool.release(buf),
                        Err(bufs) => {
                            for buf in bufs {
                                application.ctx.platform.buf_pool.release(buf);
                            }
                            client_connections[i as usize] = None;
                        }
                    }
                }
            }
            EventSource::Process(i) => {
                if let Some(process) = &mut processes[i as usize] {
                    if let Some(pipe) = &mut process.stdout {
                        let tag = process.tag;
                        match pipe.read_async(&mut application.ctx.platform.buf_pool) {
                            Ok(None) => (),
                            Ok(Some(buf)) => events.push(PlatformEvent::ProcessOutput { tag, buf }),
                            Err(()) => {
                                process.stdout = None;
                                process.kill();
                                processes[i as usize] = None;
                                events.push(PlatformEvent::ProcessExit { tag });
                            }
                        }
                    }
                }
            }
            EventSource::IpcRead(i) => {
                if let Some(ipc) = &mut ipcs[i as usize] {
                    let tag = ipc.tag;
                    match ipc.read_async(&mut application.ctx.platform.buf_pool) {
                        Ok(None) => (),
                        Ok(Some(buf)) => {
                            events.push(PlatformEvent::IpcOutput { tag, buf });
                        }
                        Err(()) => {
                            ipc.dispose(&mut application.ctx.platform.buf_pool);
                            ipcs[i as usize] = None;
                            events.push(PlatformEvent::IpcClose { tag });
                        }
                    }
                }
            }
            EventSource::IpcWrite(i) => {
                if let Some(ipc) = &mut ipcs[i as usize] {
                    match ipc.write_pending_async() {
                        Ok(None) => (),
                        Ok(Some(buf)) => application.ctx.platform.buf_pool.release(buf),
                        Err(bufs) => {
                            for buf in bufs {
                                application.ctx.platform.buf_pool.release(buf);
                            }
                            ipcs[i as usize] = None;
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
    handle: Handle,
    reader: AsyncIO,
    buf: Box<[u8; CLIENT_CONNECTION_BUFFER_LEN]>,
}
impl ConnectionToServer {
    pub fn connect(path: &[u16]) -> Self {
        let handle = match create_file(path, GENERIC_READ | GENERIC_WRITE, 0, FILE_FLAG_OVERLAPPED)
        {
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

        let reader = AsyncIO::new(handle.0);
        let buf = Box::new([0; CLIENT_CONNECTION_BUFFER_LEN]);

        Self {
            handle,
            reader,
            buf,
        }
    }

    pub fn event(&self) -> &Event {
        self.reader.event()
    }

    pub fn write(&mut self, buf: &[u8]) -> bool {
        write_all_bytes(&self.handle, buf)
    }

    pub fn read_async(&mut self) -> Result<&[u8], ()> {
        match self.reader.read_async(&mut self.buf[..]) {
            IoResult::Waiting => Ok(&[]),
            IoResult::Ok(0) | IoResult::Err => Err(()),
            IoResult::Ok(len) => Ok(&self.buf[..len]),
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
            let handle = create_file(&path, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ, 0);
            if handle.is_none() {
                panic!("could not open console input");
            }
            handle
        };
        console_output_handle = {
            let path = b"CONOUT$\0".map(|b| b as _);
            let handle = create_file(&path, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_WRITE, 0);
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
        let (_, bytes) = application.update(Some(size), &[Key::default()], None, &[]);
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
            1 => keys.push(Key {
                code: KeyCode::Char('c'),
                shift: false,
                control: true,
                alt: false,
            }),
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
    resize: &mut Option<(u16, u16)>,
) {
    fn decode_utf16(previous_codepoint: &mut Option<u16>, current_codepoint: u16) -> Option<char> {
        let codepoints = previous_codepoint
            .take()
            .into_iter()
            .chain(std::iter::once(current_codepoint));
        match std::char::decode_utf16(codepoints).next() {
            Some(Ok(c)) => Some(c),
            _ => {
                *previous_codepoint = Some(current_codepoint);
                None
            }
        }
    }

    let mut previous_unicode_char = None;

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

                let mut shift = control_key_state & SHIFT_PRESSED != 0;
                let control = control_key_state & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0;
                let alt = control_key_state & (LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED) != 0;

                const CHAR_A: i32 = b'A' as _;
                const CHAR_Z: i32 = b'Z' as _;
                let code = match keycode {
                    VK_BACK => KeyCode::Backspace,
                    VK_RETURN => KeyCode::Char('\n'),
                    VK_LEFT => KeyCode::Left,
                    VK_RIGHT => KeyCode::Right,
                    VK_UP => KeyCode::Up,
                    VK_DOWN => KeyCode::Down,
                    VK_HOME => KeyCode::Home,
                    VK_END => KeyCode::End,
                    VK_PRIOR => KeyCode::PageUp,
                    VK_NEXT => KeyCode::PageDown,
                    VK_TAB => KeyCode::Char('\t'),
                    VK_DELETE => KeyCode::Delete,
                    VK_F1..=VK_F24 => KeyCode::F((keycode - VK_F1 + 1) as _),
                    VK_ESCAPE => KeyCode::Esc,
                    VK_SPACE => match decode_utf16(&mut previous_unicode_char, unicode_char) {
                        Some(c) => KeyCode::Char(c),
                        None => continue,
                    },
                    CHAR_A..=CHAR_Z => {
                        let mut c = if control || alt {
                            ((keycode - CHAR_A) as u8 + b'a') as char
                        } else {
                            match decode_utf16(&mut previous_unicode_char, unicode_char) {
                                Some(c) => c,
                                None => continue,
                            }
                        };
                        if shift {
                            c = c.to_ascii_uppercase();
                        } else {
                            shift = c.is_ascii_uppercase();
                        }
                        KeyCode::Char(c)
                    }
                    _ => match decode_utf16(&mut previous_unicode_char, unicode_char) {
                        Some(c) => {
                            shift = false;
                            KeyCode::Char(c)
                        }
                        _ => continue,
                    },
                };

                if let KeyCode::Char('\0') = code {
                    continue;
                }

                let key = Key {
                    code,
                    shift,
                    control,
                    alt,
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

