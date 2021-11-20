use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use pepper::application::{ApplicationConfig, ClientApplication, ServerApplication};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);
}

pub struct Application {
    server: ServerApplication,
    client: ClientApplication<Vec<u8>>,
}

#[wasm_bindgen]
pub fn pepper_new_application() -> *mut Application {
    let config = ApplicationConfig::default();
    let server = ServerApplication::new(config).unwrap();
    let mut client = ClientApplication::new();
    client.output = Some(Vec::new());

    let app = Application { server, client };
    Box::into_raw(Box::new(app))
}

#[wasm_bindgen]
pub fn pepper_init(app: *mut Application, terminal_width: u16, terminal_height: u16) -> Uint8Array {
    let app = unsafe { &mut *app };
    if let Some(output) = &mut app.client.output {
        output.clear();
    }

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

    if !key_name.is_empty() {
        //
    }

    unsafe { Uint8Array::view(app.client.output.as_ref().unwrap()) }
}
