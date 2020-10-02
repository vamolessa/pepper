// first because of macros
mod script;

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
mod navigation_history;
mod pattern;
mod picker;
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

    /// load config file at path
    #[argh(option, short = 'c')]
    config: Option<std::path::PathBuf>,

    /// session name
    #[argh(option, short = 's')]
    session: Option<String>,

    /// displays no ui and send events on behalf of the currently focused client
    #[argh(switch)]
    as_focused_client: bool,

    /// displays no ui and send events on behalf of the client at index
    #[argh(option)]
    as_client: Option<client::TargetClient>,

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
