use std::{
    ffi::CStr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};

use crate::command::{CommandContext, CommandError};

pub mod api;
mod api_impl;

pub type PluginInitFn = extern "C" fn(api: &api::PluginApi) -> api::PluginUserData;

pub fn api() -> &'static api::PluginApi {
    use api_impl::*;
    static PLUGIN_API: api::PluginApi = api::PluginApi {
        set_deinit_fn,
        register_command,
        write_to_statusbar,
    };
    &PLUGIN_API
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

pub struct Plugin {
    pub userdata: api::PluginUserData,
    pub deinit_fn: Option<api::PluginDeinitFn>,
}

static CURRENT_COMMAND_CONTEXT_PTR: AtomicPtr<usize> = AtomicPtr::new(std::ptr::null_mut());
static CURRENT_PLUGIN_HANDLE: AtomicU32 = AtomicU32::new(0);

pub fn ctx<'ctx, 'state, 'command>() -> (&'ctx mut CommandContext<'state, 'command>, PluginHandle) {
    let ctx = CURRENT_COMMAND_CONTEXT_PTR.load(Ordering::Relaxed) as *mut CommandContext;
    let ctx = unsafe { &mut *ctx };
    let handle = CURRENT_PLUGIN_HANDLE.load(Ordering::Relaxed);
    let handle = PluginHandle(handle);
    (ctx, handle)
}

fn ctx_scope<F, R>(ctx: &mut CommandContext, handle: PluginHandle, f: F) -> R
where
    F: FnOnce() -> R,
{
    let ctx = ctx as *mut _ as *mut _;
    CURRENT_COMMAND_CONTEXT_PTR.store(ctx, Ordering::Relaxed);
    CURRENT_PLUGIN_HANDLE.store(handle.0, Ordering::Relaxed);
    f()
}

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Plugin>,
}
impl PluginCollection {
    pub fn load(ctx: &mut CommandContext, init_fn: PluginInitFn) {
        let handle = PluginHandle(ctx.editor.plugins.plugins.len() as _);
        let userdata = ctx_scope(ctx, handle, move || init_fn(api()));
        ctx.editor.plugins.plugins.push(Plugin {
            userdata,
            deinit_fn: None,
        });
    }

    pub fn call_command_fn(
        ctx: &mut CommandContext,
        handle: PluginHandle,
        command_fn: api::PluginCommandFn,
    ) -> Result<(), CommandError> {
        let userdata = ctx.editor.plugins.get(handle).userdata;
        let error = ctx_scope(ctx, handle, move || command_fn(api(), userdata));
        if error.is_null() {
            Ok(())
        } else {
            match unsafe { CStr::from_ptr(error) }.to_str() {
                Ok(error) => Err(CommandError::PluginError(error)),
                Err(_) => Err(CommandError::ErrorMessageNotUtf8),
            }
        }
    }

    pub fn get(&self, handle: PluginHandle) -> &Plugin {
        &self.plugins[handle.0 as usize]
    }

    pub fn get_mut(&mut self, handle: PluginHandle) -> &mut Plugin {
        &mut self.plugins[handle.0 as usize]
    }
}
impl Drop for PluginCollection {
    fn drop(&mut self) {
        for plugin in &self.plugins {
            if let Some(deinit_fn) = plugin.deinit_fn {
                deinit_fn(plugin.userdata);
            }
        }
    }
}
