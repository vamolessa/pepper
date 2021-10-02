use std::os::raw::{c_uint};

use crate::{command::CommandContext, editor_utils::MessageKind, plugin::api::{StringSlice, PluginCommandFn}};

pub extern "C" fn register_command(
    ctx: &mut CommandContext,
    name: StringSlice,
    command_fn: PluginCommandFn,
) {
    let name = helper::to_str(name);
    ctx.editor
        .commands
        .register_plugin_command(ctx.plugin_handle, name, &[], command_fn);
}

pub extern "C" fn write_to_statusbar(
    ctx: &mut CommandContext,
    level: c_uint,
    message: StringSlice,
) {
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

    pub fn to_str<'a>(s: StringSlice) -> &'a str {
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

