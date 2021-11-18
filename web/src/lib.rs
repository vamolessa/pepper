use wasm_bindgen::prelude::*;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    pub fn alert(s: &str);
}

pub struct Application {
    terminal_size: (u16, u16),
}

#[wasm_bindgen]
pub fn new_pepper_editor(terminal_width: u16, terminal_height: u16) -> *mut Application {
    let app = Application {
        terminal_size: (terminal_width, terminal_height),
    };
    Box::into_raw(Box::new(app))
}

#[wasm_bindgen]
pub fn on_key(app: *mut Application, key: &str) {
    //&[]
}


