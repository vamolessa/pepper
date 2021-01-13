use winapi::{
    shared::minwindef::{BOOL, DWORD, FALSE, TRUE},
    um::{
        consoleapi::{
            GetConsoleMode, GetNumberOfConsoleInputEvents, ReadConsoleInputW,
            SetConsoleCtrlHandler, SetConsoleMode,
        },
        processenv::GetStdHandle,
        winbase::{STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
        wincon::{
            ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
        },
        wincontypes::{
            INPUT_RECORD, KEY_EVENT, LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED,
            RIGHT_CTRL_PRESSED, WINDOW_BUFFER_SIZE_EVENT,
        },
        winuser::{
            VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2,
            VK_F20, VK_F21, VK_F22, VK_F23, VK_F24, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7,
            VK_F8, VK_F9, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
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

    // initialize
    if SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) == FALSE {
        panic!("could not set ctrl handler");
    }

    let input_handle = GetStdHandle(STD_INPUT_HANDLE);
    let output_handle = GetStdHandle(STD_OUTPUT_HANDLE);

    let mut original_input_mode = DWORD::default();
    if GetConsoleMode(input_handle, &mut original_input_mode as _) == FALSE {
        panic!("could not retrieve original console input mode");
    }
    if SetConsoleMode(input_handle, ENABLE_WINDOW_INPUT) == FALSE {
        panic!("could not set console input mode");
    }

    let mut original_output_mode = DWORD::default();
    if GetConsoleMode(output_handle, &mut original_output_mode as _) == FALSE {
        panic!("could not retrieve original console output mode");
    }
    if SetConsoleMode(
        output_handle,
        ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    ) == FALSE
    {
        panic!("could not set console output mode");
    }

    // state
    let mut event_buffer = [INPUT_RECORD::default(); 32];

    // update
    for _ in 0..10 {
        let mut event_count: DWORD = 0;
        if GetNumberOfConsoleInputEvents(input_handle, &mut event_count as _) == FALSE {
            panic!("could not read console event count");
        }

        if event_count == 0 {
            winapi::um::synchapi::Sleep(100);
            continue;
        }

        let mut event_count: DWORD = 0;
        if ReadConsoleInputW(
            input_handle,
            (&mut event_buffer[..]).as_mut_ptr(),
            event_buffer.len() as _,
            &mut event_count as _,
        ) == FALSE
        {
            panic!("could not read console events");
        }

        for i in 0..event_count {
            let event = event_buffer[i as usize];
            match event.EventType {
                KEY_EVENT => {
                    let event = event.Event.KeyEvent();
                    if event.bKeyDown == TRUE {
                        match event.dwControlKeyState {
                            _ => (),
                        }
                        match event.wVirtualKeyCode {
                            _ => (),
                        }
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

        println!("have {} events", event_count);
    }

    println!("hello\nnew line");

    // shutdown
    SetConsoleMode(input_handle, original_input_mode);
    SetConsoleMode(output_handle, original_output_mode);
}

struct WindowsPlatform {
    //
}

impl Platform for WindowsPlatform {
    //
}
