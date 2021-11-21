use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use pepper::{
    application::{ApplicationConfig, ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{Key, PlatformEvent, PlatformRequest, PooledBuf},
    Args,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);
}

pub struct Application {
    server: ServerApplication,
    client: ClientApplication<Vec<u8>>,
    events: Vec<PlatformEvent>,
}

const CLIENT_HANDLE: ClientHandle = ClientHandle::from_raw(0);

#[wasm_bindgen]
pub fn pepper_new_application() -> *mut Application {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

    let config = ApplicationConfig::default();
    let server = ServerApplication::new(config).unwrap();
    let mut client = ClientApplication::new();
    client.output = Some(Vec::new());

    let app = Application {
        server,
        client,
        events: Vec::new(),
    };
    Box::into_raw(Box::new(app))
}

#[wasm_bindgen]
pub fn pepper_init(app: *mut Application, terminal_width: u16, terminal_height: u16) -> Uint8Array {
    let app = unsafe { &mut *app };
    if let Some(output) = &mut app.client.output {
        output.clear();
    }

    app.events.push(PlatformEvent::ConnectionOpen { handle: CLIENT_HANDLE });

    let bytes = app.client.init(Args::default());
    let buf = app.server.ctx.platform.buf_pool.acquire();
    enqueue_client_bytes(&mut app.events, buf, bytes);

    let (_, bytes) = app.client.update(
        Some((terminal_width, terminal_height)),
        &[Key::None],
        None,
        &[],
    );
    let buf = app.server.ctx.platform.buf_pool.acquire();
    enqueue_client_bytes(&mut app.events, buf, bytes);

    process_requests(app);

    unsafe { Uint8Array::view(app.client.output.as_ref().unwrap()) }
}

#[wasm_bindgen]
pub fn pepper_on_event(
    app: *mut Application,
    key_name: &str,
    key_ctrl: bool,
    key_alt: bool,
) -> Uint8Array {
    let app = unsafe { &mut *app };
    if let Some(output) = &mut app.client.output {
        output.clear();
    }

    let key = parse_key(key_name, key_ctrl, key_alt);
    if key != Key::None {
        let (_, bytes) = app.client.update(None, &[key], None, &[]);
        let buf = app.server.ctx.platform.buf_pool.acquire();
        enqueue_client_bytes(&mut app.events, buf, bytes);
        process_requests(app);
    }

    unsafe { Uint8Array::view(app.client.output.as_ref().unwrap()) }
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
        let mut requests = app.server.ctx.platform.requests.drain();
        while let Some(request) = requests.next() {
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
            }
        }
    }
}

fn parse_key(name: &str, has_ctrl: bool, has_alt: bool) -> Key {
    match name {
        "" => Key::None,
        "Backspace" => Key::Backspace,
        "Enter" => Key::Enter,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Tab" => Key::Tab,
        "Delete" => Key::Delete,
        "F1" => Key::F(1),
        "F2" => Key::F(2),
        "F3" => Key::F(3),
        "F4" => Key::F(4),
        "F5" => Key::F(5),
        "F6" => Key::F(6),
        "F7" => Key::F(7),
        "F8" => Key::F(8),
        "F9" => Key::F(9),
        "F10" => Key::F(10),
        "F11" => Key::F(11),
        "F12" => Key::F(12),
        "Escape" => Key::Esc,
        _ => {
            let mut chars = name.chars();
            match chars.next() {
                Some(c) => match chars.next() {
                    Some(_) => Key::None,
                    None => {
                        if has_ctrl {
                            Key::Ctrl(c)
                        } else if has_alt {
                            Key::Alt(c)
                        } else {
                            Key::Char(c)
                        }
                    }
                },
                None => Key::None,
            }
        }
    }
}

