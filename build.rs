use std::env;

fn main() {
    const PLUGIN_API_HEADER_FILE: &str = "pepper_plugin_api.h";

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(cbindgen::Language::C)
        .with_tab_width(4)
        .with_line_length(100)
        .with_include_guard("PEPPER_PLUGIN_API_H")
        .with_no_includes()
        .with_documentation(true)
        .with_style(cbindgen::Style::Both)
        .with_item_prefix("Pepper")
        .include_item("PluginApi")
        .include_item("PluginCommandFn")
        .with_parse_deps(false)
        .generate()
        .expect("unable to generate plugin api bindings")
        .write_to_file(PLUGIN_API_HEADER_FILE);
}
