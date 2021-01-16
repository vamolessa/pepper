use std::time::Duration;

use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, FALSE, TRUE},
        ntdef::NULL,
        winerror::{ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, WAIT_TIMEOUT},
    },
    um::{
        consoleapi::{GetConsoleMode, ReadConsoleInputW, SetConsoleCtrlHandler, SetConsoleMode},
        errhandlingapi::GetLastError,
        fileapi::{CreateFileW, ReadFile, WriteFile, OPEN_EXISTING},
        handleapi::INVALID_HANDLE_VALUE,
        ioapiset::GetOverlappedResult,
        minwinbase::OVERLAPPED,
        namedpipeapi::{ConnectNamedPipe, CreateNamedPipeW, SetNamedPipeHandleState},
        processenv::GetStdHandle,
        synchapi::{CreateEventW, SetEvent, WaitForMultipleObjects},
        winbase::{
            FILE_FLAG_OVERLAPPED, INFINITE, PIPE_ACCESS_DUPLEX, PIPE_READMODE_MESSAGE,
            PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            WAIT_ABANDONED_0, WAIT_FAILED, WAIT_OBJECT_0,
        },
        wincon::{
            ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
        },
        wincontypes::{
            INPUT_RECORD, KEY_EVENT, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED,
            RIGHT_CTRL_PRESSED, SHIFT_PRESSED, WINDOW_BUFFER_SIZE_EVENT,
        },
        winnt::{GENERIC_READ, GENERIC_WRITE, HANDLE},
        winuser::{
            VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_LEFT,
            VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
        },
    },
};

use crate::platform::{Key, Platform};

pub fn run() {
    unsafe { run_unsafe() }
}

unsafe fn run_unsafe() {
    unsafe extern "system" fn ctrl_handler(_ctrl_type: DWORD) -> BOOL {
        FALSE
    }

    if SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) == FALSE {
        panic!("could not set ctrl handler");
    }

    let session_name = "session_name";
    let mut pipe_path = Vec::new();
    pipe_path.extend("\\\\.\\pipe\\".encode_utf16());
    pipe_path.extend(session_name.encode_utf16());
    pipe_path.push(0);

    if !try_run_client(&pipe_path) {
        run_server(&pipe_path);
    }
}

enum WaitResult {
    Signaled(usize),
    Abandoned(usize),
    Timeout,
}
unsafe fn wait_for_multiple_objects(
    handles: &mut [HANDLE],
    timeout: Option<Duration>,
) -> WaitResult {
    let timeout = match timeout {
        Some(duration) => duration.as_millis() as _,
        None => INFINITE,
    };
    let result = WaitForMultipleObjects(handles.len() as _, handles.as_mut_ptr(), FALSE, timeout);
    let len = handles.len() as DWORD;
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

enum ReadResult {
    Pending,
    Ok(usize),
    Err,
}
enum WriteResult {
    Ok,
    Err,
}
struct NamedPipe {
    pipe_handle: HANDLE,
    overlapped: OVERLAPPED,
    event_handle: HANDLE,
    pending_io: bool,
}
impl NamedPipe {
    pub unsafe fn create(path: &[u16], buffer_len: usize) -> Self {
        let event_handle = CreateEventW(std::ptr::null_mut(), TRUE, TRUE, std::ptr::null());
        if event_handle == NULL {
            panic!("could not create new connection");
        }

        let pipe_handle = CreateNamedPipeW(
            path.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE,
            PIPE_UNLIMITED_INSTANCES,
            buffer_len as _,
            buffer_len as _,
            0,
            std::ptr::null_mut(),
        );
        if pipe_handle == INVALID_HANDLE_VALUE {
            panic!("could not create new connection");
        }

        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = event_handle;

        Self {
            pipe_handle,
            overlapped,
            event_handle,
            pending_io: false,
        }
    }

    pub unsafe fn accept(&mut self) -> ReadResult {
        if ConnectNamedPipe(self.pipe_handle, &mut self.overlapped) != FALSE {
            panic!("could not accept incomming connection");
        }

        match GetLastError() {
            ERROR_IO_PENDING => {
                self.pending_io = true;
                ReadResult::Pending
            }
            ERROR_PIPE_CONNECTED => {
                self.pending_io = false;
                if SetEvent(self.event_handle) == FALSE {
                    panic!("could not accept incomming connection");
                }
                ReadResult::Ok(0)
            }
            _ => ReadResult::Err,
        }
    }

    pub unsafe fn try_connect(path: &[u16]) -> Option<Self> {
        let pipe_handle = CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            NULL,
        );
        if pipe_handle == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut mode = PIPE_READMODE_MESSAGE;
        if SetNamedPipeHandleState(
            pipe_handle,
            &mut mode,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) == FALSE
        {
            panic!("could not establish a connection");
        }

        let event_handle = CreateEventW(std::ptr::null_mut(), TRUE, FALSE, std::ptr::null());
        if event_handle == NULL {
            panic!("could not connect to server");
        }

        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = event_handle;

        Some(Self {
            pipe_handle,
            overlapped,
            event_handle,
            pending_io: false,
        })
    }

    pub unsafe fn read_async(&mut self, buf: &mut [u8]) -> ReadResult {
        let mut read_len = 0;
        if self.pending_io {
            if GetOverlappedResult(self.pipe_handle, &mut self.overlapped, &mut read_len, FALSE)
                == FALSE
            {
                ReadResult::Err
            } else {
                ReadResult::Ok(read_len as _)
            }
        } else {
            if ReadFile(
                self.pipe_handle,
                buf.as_mut_ptr() as _,
                buf.len() as _,
                &mut read_len,
                &mut self.overlapped,
            ) != FALSE
            {
                ReadResult::Ok(read_len as _)
            } else if GetLastError() == ERROR_IO_PENDING {
                self.pending_io = true;
                ReadResult::Pending
            } else {
                ReadResult::Err
            }
        }
    }

    pub unsafe fn write(&mut self, buf: &[u8]) -> WriteResult {
        let mut write_len = 0;
        if WriteFile(
            self.pipe_handle,
            buf.as_ptr() as _,
            buf.len() as _,
            &mut write_len,
            std::ptr::null_mut(),
        ) == FALSE
        {
            WriteResult::Err
        } else {
            WriteResult::Ok
        }
    }
}

unsafe fn run_server(pipe_path: &[u16]) {
    const PIPE_BUFFER_LEN: usize = 512;

    struct NamedPipeInstance {
        pub pipe: NamedPipe,
        pub connecting: bool,
        pub read_buf: [u8; PIPE_BUFFER_LEN],
    }
    impl NamedPipeInstance {
        pub unsafe fn new(pipe_path: &[u16]) -> Self {
            Self {
                pipe: NamedPipe::create(pipe_path, PIPE_BUFFER_LEN),
                connecting: false,
                read_buf: [0; PIPE_BUFFER_LEN],
            }
        }

        pub unsafe fn accept(&mut self) {
            match self.pipe.accept() {
                ReadResult::Pending => self.connecting = true,
                ReadResult::Ok(_) => self.connecting = false,
                ReadResult::Err => panic!("could not accept client"),
            }
        }
    }

    let mut pipes = [NamedPipeInstance::new(pipe_path)];
    let mut wait_events = [pipes[0].pipe.event_handle];
    pipes[0].accept();

    loop {
        let pipe = match wait_for_multiple_objects(&mut wait_events, None) {
            WaitResult::Signaled(i) => &mut pipes[i],
            _ => continue,
        };

        match pipe.pipe.read_async(&mut pipe.read_buf) {
            ReadResult::Pending => (),
            ReadResult::Ok(0) | ReadResult::Err => {
                // disconnect and accept new
                panic!("CLIENT DISCONNECTED");
            }
            ReadResult::Ok(len) => {
                let message = &pipe.read_buf[..len];
                let message = String::from_utf8_lossy(message);
                println!("received {} bytes! message: '{}'", len, message);

                let message = b"thank you for your message!";
                match pipe.pipe.write(message) {
                    WriteResult::Ok => (),
                    WriteResult::Err => {
                        panic!("could not send message to client {}", GetLastError())
                    }
                }
            }
        }
    }
}

unsafe fn try_run_client(pipe_path: &[u16]) -> bool {
    let mut pipe = match NamedPipe::try_connect(pipe_path) {
        Some(pipe) => pipe,
        None => return false,
    };

    let input_handle = GetStdHandle(STD_INPUT_HANDLE);
    let output_handle = GetStdHandle(STD_OUTPUT_HANDLE);

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

    let mut pipe_buf = [0u8; 1024 * 2];
    let event_buffer = &mut [INPUT_RECORD::default(); 32][..];
    let mut wait_handles = [input_handle, pipe.event_handle];

    'main_loop: loop {
        let wait_handle_index = match wait_for_multiple_objects(&mut wait_handles, None) {
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

                            println!("key {} * {}", key, repeat_count);

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
            1 => {
                match pipe.read_async(&mut pipe_buf) {
                    ReadResult::Pending => (),
                    ReadResult::Ok(0) | ReadResult::Err => {
                        panic!("SERVER DISCONNECTED {}", GetLastError());
                    }
                    ReadResult::Ok(len) => {
                        //
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    SetConsoleMode(input_handle, original_input_mode);
    SetConsoleMode(output_handle, original_output_mode);
    true
}
