use std::os::raw::{c_char, c_int, c_void};

use crate::command::CommandContext;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PluginUserData(pub *mut c_void);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PluginCommandFn1(
    pub  extern "C" fn(
        api: &PluginApi,
        ctx: &mut CommandContext,
        userdata: PluginUserData,
    ) -> *const c_char,
);

pub type PluginCommandFn = extern "C" fn(
    api: &PluginApi,
    ctx: &mut CommandContext,
    userdata: PluginUserData,
) -> *const c_char;

#[repr(C)]
pub struct PluginApi {
    pub register_command:
        extern "C" fn(ctx: &mut CommandContext, name: *const c_char, command_fn: PluginCommandFn),

    pub write_to_statusbar:
        extern "C" fn(ctx: &mut CommandContext, level: c_int, message: *const c_char),
}

