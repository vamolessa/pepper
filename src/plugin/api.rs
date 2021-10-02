use std::os::raw::{c_char, c_void, c_uint};

use crate::command::CommandContext;

#[repr(C)]
pub struct StringSlice {
    pub bytes: *const c_char,
    pub len: c_uint,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PluginUserData(pub *mut c_void);

pub type PluginCommandFn = extern "C" fn(
    api: &PluginApi,
    ctx: &mut CommandContext,
    userdata: PluginUserData,
) -> *const c_char;

#[repr(C)]
pub struct PluginApi {
    pub register_command:
        extern "C" fn(ctx: &mut CommandContext, name: StringSlice, command_fn: PluginCommandFn),
    pub write_to_statusbar:
        extern "C" fn(ctx: &mut CommandContext, level: c_uint, message: StringSlice),
}
