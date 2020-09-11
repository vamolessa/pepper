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
mod keymap;
mod mode;
mod pattern;
mod script;
mod script_bindings;
mod picker;
mod word_database;
mod serialization;
mod syntax;
mod theme;
mod tui;

use argh::FromArgs;

/// Pepper editor is a minimalistic and modal code editor inspired by vim and kakoune.
#[derive(FromArgs)]
pub struct Args {
    /// print version and quit
    #[argh(switch, short = 'v')]
    version: bool,

    /// load config file at path
    #[argh(option, short = 'c')]
    config: Option<std::path::PathBuf>,

    /// session name
    #[argh(option, short = 's')]
    session: Option<String>,

    /// send events on behalf of the currently focused client
    #[argh(switch)]
    as_focused_client: bool,

    /// send events on behalf of the client at index
    #[argh(option)]
    as_client: Option<usize>,

    /// send keys to server and quit
    #[argh(option, short = 'k')]
    keys: Option<String>,

    /// open files at paths
    #[argh(positional)]
    files: Vec<String>,
}

fn main() {
    let args: Args = argh::from_env();
    if args.version {
        let name = env!("CARGO_PKG_NAME");
        let version = env!("CARGO_PKG_VERSION");
        println!("{} version {}", name, version);
    } else if let Err(e) = application::run(args) {
        eprintln!("{}", e);
    }
}
