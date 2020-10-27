mod macros;

mod application;
mod buffer;
mod buffer_position;
mod buffer_view;
mod client;
mod client_event;
mod config;
mod connection;
mod cursor;
mod editor;
mod event_manager;
mod history;
mod json;
mod keymap;
mod lsp;
mod mode;
mod navigation_history;
mod pattern;
mod picker;
mod register;
mod script;
mod script_bindings;
mod serialization;
mod syntax;
mod theme;
mod ui;
mod word_database;

use argh::FromArgs;

/// Pepper editor is a minimalistic and modal code editor inspired by vim and kakoune.
#[derive(FromArgs)]
pub struct Args {
    /// print version and quit
    #[argh(switch, short = 'v')]
    version: bool,

    /// load config file at path (repeatable)
    #[argh(option, short = 'c')]
    config: Vec<std::path::PathBuf>,

    /// adds an extra script module search path (repeatable)
    #[argh(option)]
    module_search_path: Vec<std::path::PathBuf>,

    /// session name
    #[argh(option, short = 's')]
    session: Option<String>,

    /// displays no ui and send events on behalf of the currently focused client
    #[argh(switch)]
    as_focused_client: bool,

    /// displays no ui and send events on behalf of the client at index
    #[argh(option)]
    as_client: Option<client::TargetClient>,

    #[argh(switch)]
    /// will print to stderr frames latency
    profile: bool,

    /// open files at paths
    /// you can append ':<line-number>' to a path to open it at that line
    #[argh(positional)]
    files: Vec<String>,
}

fn main() {
    let mut lsp_client = lsp::LspClient::new("rust-analyzer").unwrap();
    lsp_client.initialize().unwrap();
    println!("response:\n{}", lsp_client.wait_response().unwrap());
    return;

    let args: Args = argh::from_env();
    if args.version {
        let name = env!("CARGO_PKG_NAME");
        let version = env!("CARGO_PKG_VERSION");
        println!("{} version {}", name, version);
    } else if let Err(e) = application::run(args) {
        eprintln!("{}", e);
    }
}
