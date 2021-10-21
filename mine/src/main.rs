use std::path::Path;

use pepper::{buffer::BufferProperties, help::HelpPages, plugin::PluginDefinition, ResourceFile};

static HELP_PAGES: HelpPages = HelpPages::new(&[]);
static ALTERNATE_FILE_PLUGIN: PluginDefinition = PluginDefinition {
    instantiate: |handle, ctx| {
        ctx.editor
            .commands
            .register(Some(handle), "goto-alternate-buffer", &[], |ctx, io| {
                let client = ctx.clients.get_mut(io.client_handle()?);

                let buffer_view_handle = match client.buffer_view_handle() {
                    Some(handle) => handle,
                    None => return Ok(()),
                };

                let buffer_handle = ctx
                    .editor
                    .buffer_views
                    .get(buffer_view_handle)
                    .buffer_handle;

                let path = &ctx.editor.buffers.get(buffer_handle).path;
                let path = match path.to_str() {
                    Some(path) => path,
                    None => return Ok(()),
                };

                let (rest, try_extensions) = if let Some(rest) = path.strip_suffix(".c") {
                    (rest, [".h", ".hpp"])
                } else if let Some(rest) = path.strip_suffix(".cpp") {
                    (rest, [".h", ".hpp"])
                } else if let Some(rest) = path.strip_suffix(".h") {
                    (rest, [".c", ".cpp"])
                } else if let Some(rest) = path.strip_suffix(".hpp") {
                    (rest, [".c", ".cpp"])
                } else {
                    return Ok(());
                };

                let mut path = ctx.editor.string_pool.acquire_with(rest);
                let path_len = path.len();
                for ext in try_extensions {
                    path.push_str(ext);

                    if let Ok(buffer_view_handle) = ctx.editor.buffer_view_handle_from_path(
                        client.handle(),
                        Path::new(&path),
                        BufferProperties::text(),
                        false,
                    ) {
                        client.set_buffer_view_handle(
                            Some(buffer_view_handle),
                            &ctx.editor.buffer_views,
                        );
                        break;
                    }

                    path.truncate(path_len);
                }
                ctx.editor.string_pool.release(path);

                Ok(())
            });

        None
    },
    help_pages: &HELP_PAGES,
};

fn main() {
    let mut config = pepper::application::ApplicationConfig::default();
    config.on_panic_config.write_info_to_file = Some(Path::new("pepper-crash.txt"));
    config.on_panic_config.try_attaching_debugger = true;

    config.plugin_definitions.push(ALTERNATE_FILE_PLUGIN);
    config
        .plugin_definitions
        .push(pepper_plugin_lsp::DEFINITION);
    config
        .plugin_definitions
        .push(pepper_plugin_unreal::DEFINITION);

    config
        .static_configs
        .push(pepper_plugin_lsp::DEFAULT_BINDINGS_CONFIG);
    config
        .static_configs
        .push(pepper_plugin_unreal::DEFAULT_BINDINGS_CONFIG);
    config.static_configs.push(ResourceFile {
        name: "my.pepper",
        content: "map-normal ga [[: goto-alternate-buffer<enter>]]",
    });

    pepper::run(config);
}
