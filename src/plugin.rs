use std::{
    os::raw::{c_int, c_void},
    process,
};

use crate::command::{CommandContext, PluginCommandFn};

pub type InitPluginFn = extern "C" fn(api: &PluginApi, ctx: &mut CommandContext) -> *const c_void;

pub struct PluginApi {
    pub register_command: extern "C" fn(
        ctx: &mut CommandContext,
        name: *const u8,
        name_len: c_int,
        completions: *const c_int,
        completions_len: c_int,
        command_fn: PluginCommandFn,
    ),
}

pub static PLUGIN_API: PluginApi = PluginApi {
    register_command,
    //
};

extern "C" fn register_command(
    ctx: &mut CommandContext,
    name: *const u8,
    name_len: c_int,
    completions: *const c_int,
    completions_len: c_int,
    command_fn: PluginCommandFn,
) {
    let name = to_str(name, name_len);
    let completions = &[];
    ctx.editor
        .commands
        .register_plugin_command(ctx.plugin_handle, name, completions, command_fn);
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

fn to_str<'a>(ptr: *const u8, len: c_int) -> &'a str {
    if ptr.is_null() || len < 0 {
        process::abort();
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as _) };
    match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => process::abort(),
    }
}

fn plugin_init_test(api: &PluginApi) -> c_int {
    //let result = (api.hello)();
    //eprintln!("hello {}", result);
    true as _
}

