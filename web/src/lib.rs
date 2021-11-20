use wasm_bindgen::prelude::*;
use js_sys::Uint8Array;

use pepper::application::{ApplicationConfig, ServerApplication, ClientApplication};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);
}

pub struct Application {
    server: ServerApplication,
    client: ClientApplication,
    display_buf: Vec<u8>,
}

#[wasm_bindgen]
pub fn pepper_new_application() -> *mut Application {
    let config = ApplicationConfig::default();
    let server = ServerApplication::new(config).unwrap();
    let client = ClientApplication::new(None);

    let app = Application {
        server,
        client,
        display_buf: Vec::new(),
    };
    Box::into_raw(Box::new(app))
}

#[wasm_bindgen]
pub fn pepper_init(app: *mut Application, terminal_width: u16, terminal_height: u16) -> Uint8Array {
    let app = unsafe { &mut *app };
    app.display_buf.clear();

    unsafe { Uint8Array::view(&app.display_buf) }
}

#[wasm_bindgen]
pub fn pepper_on_event(app: *mut Application, key_name: &str, key_ctrl: bool, key_alt: bool) -> Uint8Array {
    let app = unsafe { &mut *app };
    app.display_buf.clear();

    if !key_name.is_empty() {
        //
    }

    unsafe { Uint8Array::view(&app.display_buf) }
}

