use pepper::{
    application::{ApplicationConfig, ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{Key, KeyCode, PlatformEvent, PlatformRequest, PooledBuf},
    Args,
};

extern "C" {
    fn console_error(message_ptr: *const u8, message_len: usize);
}

pub struct Application {
    server: ServerApplication,
    client: ClientApplication<Vec<u8>>,
    events: Vec<PlatformEvent>,
}

fn panic_hook(info: &std::panic::PanicInfo) {
    let mut msg = info.to_string();
    msg.push_str("\n\n");

    unsafe {
        console_error(msg.as_ptr(), msg.len());
    }
}

#[no_mangle]
pub extern "C" fn pepper_init(terminal_width: u16, terminal_height: u16) -> *mut Application {
    std::panic::set_hook(Box::new(panic_hook));

    let config = ApplicationConfig::default();
    let server = ServerApplication::new(config).unwrap();
    let mut client = ClientApplication::new();
    client.output = Some(Vec::new());

    let app = Application {
        server,
        client,
        events: Vec::new(),
    };
    let mut app = Box::new(app);

    if let Some(output) = &mut app.client.output {
        output.clear();
    }

    app.events.push(PlatformEvent::ConnectionOpen {
        handle: CLIENT_HANDLE,
    });

    let bytes = app.client.init(Args::default());
    let buf = app.server.ctx.platform.buf_pool.acquire();
    enqueue_client_bytes(&mut app.events, buf, bytes);

    let (_, bytes) = app.client.update(
        Some((terminal_width, terminal_height)),
        &[Key::default()],
        None,
        &[],
    );
    let buf = app.server.ctx.platform.buf_pool.acquire();
    enqueue_client_bytes(&mut app.events, buf, bytes);

    process_requests(&mut app);

    Box::into_raw(app)
}

const CLIENT_HANDLE: ClientHandle = ClientHandle(0);

#[no_mangle]
pub extern "C" fn pepper_output_ptr(app: *const Application) -> *const u8 {
    static EMPTY_OUTPUT: [u8; 0] = [];

    let app = unsafe { &*app };
    match &app.client.output {
        Some(output) => output.as_ptr(),
        None => EMPTY_OUTPUT.as_ptr(),
    }
}

#[no_mangle]
pub extern "C" fn pepper_output_len(app: *const Application) -> usize {
    let app = unsafe { &*app };
    match &app.client.output {
        Some(output) => output.len(),
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn pepper_on_event(
    app: *mut Application,
    key_kind: u32,
    key_value: u32,
    key_ctrl: bool,
    key_alt: bool,
) {
    let app = unsafe { &mut *app };
    if let Some(output) = &mut app.client.output {
        output.clear();
    }

    let key_code = match key_kind {
        1 => KeyCode::Char(char::from_u32(key_value).unwrap()),
        2 => KeyCode::Backspace,
        3 => KeyCode::Left,
        4 => KeyCode::Right,
        5 => KeyCode::Up,
        6 => KeyCode::Down,
        7 => KeyCode::Home,
        8 => KeyCode::End,
        9 => KeyCode::PageUp,
        10 => KeyCode::PageDown,
        11 => KeyCode::Delete,
        12 => KeyCode::F(key_value as _),
        13 => KeyCode::Esc,
        _ => KeyCode::None,
    };

    if key_code != KeyCode::None {
        let key_shift = match key_code {
            KeyCode::Char(c) => c.is_ascii_uppercase(),
            _ => false,
        };
        let key = Key {
            code: key_code,
            shift: key_shift,
            control: key_ctrl,
            alt: key_alt,
        };

        let (_, bytes) = app.client.update(None, &[key], None, &[]);
        let buf = app.server.ctx.platform.buf_pool.acquire();
        enqueue_client_bytes(&mut app.events, buf, bytes);
        process_requests(app);
    }
}

fn enqueue_client_bytes(events: &mut Vec<PlatformEvent>, mut buf: PooledBuf, bytes: &[u8]) {
    let write = buf.write();
    write.extend_from_slice(bytes);
    events.push(PlatformEvent::ConnectionOutput {
        handle: CLIENT_HANDLE,
        buf,
    });
}

fn process_requests(app: &mut Application) {
    while !app.events.is_empty() {
        app.server.update(app.events.drain(..));
        for request in app.server.ctx.platform.requests.drain() {
            match request {
                PlatformRequest::Quit => (),
                PlatformRequest::Redraw => (),
                PlatformRequest::WriteToClient { buf, .. } => {
                    let (_, _) = app.client.update(None, &[], None, buf.as_bytes());
                    app.server.ctx.platform.buf_pool.release(buf);
                }
                PlatformRequest::CloseClient { .. } => (),
                PlatformRequest::SpawnProcess { tag, .. } => {
                    app.events.push(PlatformEvent::ProcessExit { tag });
                }
                PlatformRequest::WriteToProcess { buf, .. } => {
                    app.server.ctx.platform.buf_pool.release(buf);
                }
                PlatformRequest::CloseProcessInput { .. } => (),
                PlatformRequest::KillProcess { .. } => (),
                PlatformRequest::ConnectToIpc { path, .. } => {
                    app.server.ctx.platform.buf_pool.release(path);
                }
                PlatformRequest::WriteToIpc { buf, .. } => {
                    app.server.ctx.platform.buf_pool.release(buf);
                }
                PlatformRequest::CloseIpc { .. } => (),
            }
        }
    }
}
