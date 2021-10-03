use std::os::raw::c_uint;

use crate::{
    editor_utils::MessageKind,
    plugin::{
        api::{ByteSlice, PluginCommandFn, PluginDeinitFn, PluginEventHandlerFn},
        ctx,
    },
};

pub extern "C" fn set_deinit_fn(deinit_fn: PluginDeinitFn) {
    let (ctx, handle) = ctx();
    ctx.editor.plugins.get_mut(handle).deinit_fn = Some(deinit_fn);
}

pub extern "C" fn set_event_handler_fn(event_handler_fn: PluginEventHandlerFn) {
    let (ctx, handle) = ctx();
    ctx.editor.plugins.get_mut(handle).event_handler_fn = Some(event_handler_fn);
}

pub extern "C" fn register_command(name: ByteSlice, command_fn: PluginCommandFn) {
    let (ctx, handle) = ctx();
    let name = helper::to_str(name);
    ctx.editor
        .commands
        .register_plugin_command(handle, name, &[], command_fn);
}

pub extern "C" fn write_to_statusbar(level: c_uint, message: ByteSlice) {
    let (ctx, _) = ctx();
    let kind = match level {
        0 => MessageKind::Info,
        1 => MessageKind::Error,
        _ => return,
    };
    let message = helper::to_str(message);
    ctx.editor.status_bar.write(kind).str(message);
}

mod helper {
    use super::*;

    pub fn abort(message: &str) -> ! {
        eprintln!("{}", message);
        std::process::abort();
    }

    pub fn to_str<'a>(s: ByteSlice) -> &'a str {
        if s.bytes.is_null() {
            abort("tried to dereference null ptr as &str");
        }

        let bytes = unsafe { std::slice::from_raw_parts(s.bytes as _, s.len as _) };
        match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => abort("invalid c string"),
        }
    }
}

