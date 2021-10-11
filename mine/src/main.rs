use std::path::Path;

fn main() {
    let mut config = pepper::application::ApplicationConfig::default();
    config.on_panic_config.write_info_to_file = Some(Path::new("pepper-crash.txt"));
    config.on_panic_config.try_attaching_debugger = true;

    config
        .plugin_definitions
        .push(pepper_plugin_lsp::DEFINITION);

    pepper::run(config);
}
