use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use pepper::{
    application::{ApplicationConfig, ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{AnsiKey, PlatformEvent, PlatformRequest, PooledBuf},
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

const CLIENT_HANDLE: ClientHandle = ClientHandle(0);

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

    app.events.push(PlatformEvent::ConnectionOpen {
        handle: CLIENT_HANDLE,
    });

    let bytes = app.client.init(Args::default());
    let buf = app.server.ctx.platform.buf_pool.acquire();
    enqueue_client_bytes(&mut app.events, buf, bytes);

    let (_, bytes) = app.client.update(
        Some((terminal_width, terminal_height)),
        &[AnsiKey::None],
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
    if key != AnsiKey::None {
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

fn parse_key(name: &str, has_ctrl: bool, has_alt: bool) -> AnsiKey {
    match name {
        "" => AnsiKey::None,
        "Backspace" => AnsiKey::Backspace,
        "Enter" => AnsiKey::Enter,
        "ArrowLeft" => AnsiKey::Left,
        "ArrowRight" => AnsiKey::Right,
        "ArrowUp" => AnsiKey::Up,
        "ArrowDown" => AnsiKey::Down,
        "Home" => AnsiKey::Home,
        "End" => AnsiKey::End,
        "PageUp" => AnsiKey::PageUp,
        "PageDown" => AnsiKey::PageDown,
        "Tab" => AnsiKey::Tab,
        "Delete" => AnsiKey::Delete,
        "F1" => AnsiKey::F(1),
        "F2" => AnsiKey::F(2),
        "F3" => AnsiKey::F(3),
        "F4" => AnsiKey::F(4),
        "F5" => AnsiKey::F(5),
        "F6" => AnsiKey::F(6),
        "F7" => AnsiKey::F(7),
        "F8" => AnsiKey::F(8),
        "F9" => AnsiKey::F(9),
        "F10" => AnsiKey::F(10),
        "F11" => AnsiKey::F(11),
        "F12" => AnsiKey::F(12),
        "Escape" => AnsiKey::Esc,
        _ => {
            let mut chars = name.chars();
            match chars.next() {
                Some(c) => match chars.next() {
                    Some(_) => AnsiKey::None,
                    None => {
                        if has_ctrl {
                            AnsiKey::Ctrl(c)
                        } else if has_alt {
                            AnsiKey::Alt(c)
                        } else {
                            AnsiKey::Char(c)
                        }
                    }
                },
                None => AnsiKey::None,
            }
        }
    }
}
