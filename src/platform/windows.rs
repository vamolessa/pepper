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
            RIGHT_CTRL_PRESSED, SHIFT_PRESSED, WINDOW_BUFFER_SIZE_EVENT,
        },
        winnt::HANDLE,
        winuser::{
            VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_LEFT,
            VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
        },
    },
};

use crate::platform::{Key, Platform};

struct State {
    input_handle: HANDLE,
    output_handle: HANDLE,
}

impl Platform for State {
    //
}

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

    let mut state = State {
        input_handle: GetStdHandle(STD_INPUT_HANDLE),
        output_handle: GetStdHandle(STD_OUTPUT_HANDLE),
    };

    let mut original_input_mode = DWORD::default();
    if GetConsoleMode(state.input_handle, &mut original_input_mode) == FALSE {
        panic!("could not retrieve original console input mode");
    }
    if SetConsoleMode(state.input_handle, ENABLE_WINDOW_INPUT) == FALSE {
        panic!("could not set console input mode");
    }

    let mut original_output_mode = DWORD::default();
    if GetConsoleMode(state.output_handle, &mut original_output_mode) == FALSE {
        panic!("could not retrieve original console output mode");
    }
    if SetConsoleMode(
        state.output_handle,
        ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    ) == FALSE
    {
        panic!("could not set console output mode");
    }

    // state
    let mut event_buffer = [INPUT_RECORD::default(); 32];

    // update
    'main_loop: loop {
        let mut event_count: DWORD = 0;
        if GetNumberOfConsoleInputEvents(state.input_handle, &mut event_count) == FALSE {
            panic!("could not read console event count");
        }

        if event_count == 0 {
            winapi::um::synchapi::Sleep(100);
            continue;
        }

        let mut event_count: DWORD = 0;
        if ReadConsoleInputW(
            state.input_handle,
            (&mut event_buffer[..]).as_mut_ptr(),
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
                            const ALT_PRESSED_MASK: DWORD = LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED;
                            const CTRL_PRESSED_MASK: DWORD = LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED;

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

    // shutdown
    SetConsoleMode(state.input_handle, original_input_mode);
    SetConsoleMode(state.output_handle, original_output_mode);
}
