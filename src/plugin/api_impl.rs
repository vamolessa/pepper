use std::{
    ffi::CStr,
    os::raw::{c_char, c_int},
    process,
};

use crate::{command::CommandContext, editor_utils::MessageKind, plugin::PluginCommandFn};

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
