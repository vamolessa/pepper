use std::os::raw::{c_int, c_void};

pub type InitPluginFn = fn();

pub struct PluginApi {
    pub hello: extern "C" fn() -> u64,
}

fn plugin_init_test(api: *const PluginApi) -> c_int {
    let api = unsafe { &*api };
    let result = (api.hello)();
    eprintln!("hello {}", result);
    true as _
}

