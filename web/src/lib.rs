//use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use pepper::{
    application::{ApplicationConfig, ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{Key, KeyCode, PlatformEvent, PlatformRequest, PooledBuf},
    Args,
};

#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);

    #[wasm_bindgen(js_namespace = console)]
    fn error(msg: String);

    type Error;

    #[wasm_bindgen(constructor)]
    fn new() -> Error;

    #[wasm_bindgen(structural, method, getter)]
    fn stack(error: &Error) -> String;

    #[wasm_bindgen(typescript_type = "object")]
    #[derive(Clone, Debug)]
    pub type Object;

    #[wasm_bindgen(static_method_of = Object)]
    pub fn is(value_1: &JsValue, value_2: &JsValue) -> bool;

    #[wasm_bindgen(js_namespace = WebAssembly, extends = Object, typescript_type = "WebAssembly.Memory")]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub type Memory;

    #[wasm_bindgen(method, getter, js_namespace = WebAssembly)]
    pub fn buffer(this: &Memory) -> JsValue;

    #[wasm_bindgen(extends = Object, typescript_type = Uint8Aray)]
    #[derive(Clone, Debug)]
    pub type Uint8Array;

    #[wasm_bindgen(constructor)]
    pub fn new_with_byte_offset_and_length(
        buffer: &JsValue,
        byte_offset: u32,
        length: u32,
    ) -> Uint8Array;
}

impl PartialEq for Object {
    #[inline]
    fn eq(&self, other: &Object) -> bool {
        Object::is(self.as_ref(), other.as_ref())
    }
}
impl Eq for Object {}

pub struct Application {
    server: ServerApplication,
    client: ClientApplication<Vec<u8>>,
    events: Vec<PlatformEvent>,
}

const CLIENT_HANDLE: ClientHandle = ClientHandle(0);

impl Uint8Array {
    pub unsafe fn view(rust: &[u8]) -> Self {
        let buf = wasm_bindgen::memory();
        let mem = buf.unchecked_ref::<Memory>();
        Uint8Array::new_with_byte_offset_and_length(
            &mem.buffer(),
            rust.as_ptr() as u32,
            rust.len() as u32,
        )
    }
}

fn console_hook(info: &std::panic::PanicInfo) {
    let mut msg = info.to_string();
    msg.push_str("\n\nstack:\n\n");
    let e = Error::new();
    let stack = e.stack();
    msg.push_str(&stack);
    msg.push_str("\n\n");
    error(msg);
}

#[wasm_bindgen]
pub fn pepper_new_application() -> *mut Application {
    std::panic::set_hook(Box::new(console_hook));

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
        &[Key::default()],
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
    if key.code != KeyCode::None {
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

fn parse_key(name: &str, has_ctrl: bool, has_alt: bool) -> Key {
    let code = match name {
        "" => KeyCode::None,
        "Backspace" => KeyCode::Backspace,
        "Enter" => KeyCode::Char('\n'),
        "ArrowLeft" => KeyCode::Left,
        "ArrowRight" => KeyCode::Right,
        "ArrowUp" => KeyCode::Up,
        "ArrowDown" => KeyCode::Down,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Tab" => KeyCode::Char('\t'),
        "Delete" => KeyCode::Delete,
        "F1" => KeyCode::F(1),
        "F2" => KeyCode::F(2),
        "F3" => KeyCode::F(3),
        "F4" => KeyCode::F(4),
        "F5" => KeyCode::F(5),
        "F6" => KeyCode::F(6),
        "F7" => KeyCode::F(7),
        "F8" => KeyCode::F(8),
        "F9" => KeyCode::F(9),
        "F10" => KeyCode::F(10),
        "F11" => KeyCode::F(11),
        "F12" => KeyCode::F(12),
        "Escape" => KeyCode::Esc,
        _ => {
            let mut chars = name.chars();
            match chars.next() {
                Some(c) => match chars.next() {
                    Some(_) => KeyCode::None,
                    None => KeyCode::Char(c),
                },
                None => KeyCode::None,
            }
        }
    };

    let shift = match code {
        KeyCode::Char(c) => c.is_ascii_uppercase(),
        _ => false,
    };
    Key {
        code,
        shift,
        control: has_ctrl,
        alt: has_alt,
    }
}
