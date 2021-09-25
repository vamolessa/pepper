use std::os::raw::{c_int, c_void};

pub type InitPluginFn = fn();

pub struct PluginApi {
    //pub hello: extern "C" fn() -> u64,
}

static API: PluginApi = PluginApi {
    //hello: extern "C" || 5,
};

pub fn plugin_api_ptr() -> &'static PluginApi {
    &API
}

fn plugin_init_test(api: &PluginApi) -> c_int {
    //let result = (api.hello)();
    //eprintln!("hello {}", result);
    true as _
}

