use std::{
    ffi::CStr,
    os::raw::{c_char, c_int, c_void},
    process,
};

use crate::{
    command::{CommandContext, PluginCommandFn},
    editor_utils::MessageKind,
};

pub type PluginInitFn = extern "C" fn(api: &PluginApi, ctx: &mut CommandContext) -> Plugin;
pub type PluginDeinitFn = extern "C" fn(userdata: PluginUserData);

#[repr(C)]
pub struct PluginApi {
    pub register_command:
        extern "C" fn(ctx: &mut CommandContext, name: *const c_char, command_fn: PluginCommandFn),

    pub write_to_statusbar:
        extern "C" fn(ctx: &mut CommandContext, level: c_int, message: *const c_char),
}

use api::*;
pub static PLUGIN_API: PluginApi = PluginApi {
    register_command,
    write_to_statusbar,
};

mod api {
    use super::*;

    pub extern "C" fn register_command(
        ctx: &mut CommandContext,
        name: *const c_char,
        command_fn: PluginCommandFn,
    ) {
        let name = to_str(name);
        ctx.editor
            .commands
            .register_plugin_command(ctx.plugin_handle, name, &[], command_fn);
    }

    pub extern "C" fn write_to_statusbar(
        ctx: &mut CommandContext,
        level: c_int,
        message: *const c_char,
    ) {
        let kind = match level {
            0 => MessageKind::Info,
            1 => MessageKind::Error,
            _ => return,
        };
        let message = to_str(message);
        ctx.editor.status_bar.write(kind).str(message);
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PluginUserData(pub *mut c_void);

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

pub struct Plugin {
    pub userdata: PluginUserData,
    pub deinit_fn: Option<PluginDeinitFn>,
}

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Plugin>,
}
impl PluginCollection {
    pub fn load(ctx: &mut CommandContext, init_fn: PluginInitFn) {
        let handle = PluginHandle(ctx.editor.plugins.plugins.len() as _);
        ctx.plugin_handle = handle;
        let plugin = init_fn(&PLUGIN_API, ctx);
        ctx.editor.plugins.plugins.push(plugin);
    }

    pub fn get_userdata(&self, handle: PluginHandle) -> PluginUserData {
        self.plugins[handle.0 as usize].userdata
    }
}

fn abort(message: &str) -> ! {
    eprintln!("{}", message);
    process::abort();
}

fn to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        abort("tried to dereference null ptr as &str");
    }

    let cstr = unsafe { CStr::from_ptr(ptr) };
    match cstr.to_str() {
        Ok(s) => s,
        Err(_) => abort("invalid c string"),
    }
}

