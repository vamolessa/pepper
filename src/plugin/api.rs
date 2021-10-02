use std::os::raw::{c_char, c_uint, c_void};

#[repr(C)]
pub struct StringSlice {
    pub bytes: *const c_char,
    pub len: c_uint,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PluginUserData(pub *mut c_void);

pub type PluginDeinitFn = extern "C" fn(PluginUserData);

pub type PluginCommandFn =
    extern "C" fn(api: &PluginApi, userdata: PluginUserData) -> *const c_char;

#[repr(C)]
pub struct PluginApi {
    pub set_deinit_fn: extern "C" fn(deinit_fn: PluginDeinitFn),
    pub register_command: extern "C" fn(name: StringSlice, command_fn: PluginCommandFn),
    pub write_to_statusbar: extern "C" fn(level: c_uint, message: StringSlice),
}
