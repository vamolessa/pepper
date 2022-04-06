fn main() {
    let mut config = pepper::application::ApplicationConfig::default();

    config
        .plugin_definitions
        .push(pepper_plugin_lsp::DEFINITION);

    config
        .static_configs
        .push(pepper_plugin_lsp::DEFAULT_CONFIGS);

    pepper::run(config);
}
