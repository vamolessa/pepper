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
mod editor_operation;
mod event_manager;
mod history;
mod keymap;
mod mode;
mod pattern;
mod script;
mod script_bindings;
mod select;
mod serialization;
mod syntax;
mod theme;
mod tui;

use argh::FromArgs;

/// code editor inspired by vim and kakoune
#[derive(FromArgs)]
pub struct Args {
    /// path where config file is located
    #[argh(option, short = 'c')]
    config: Option<std::path::PathBuf>,

    /// session name
    #[argh(option, short = 's')]
    session: Option<String>,

    /// send messages in behalf of the server local client
    #[argh(switch)]
    as_local_client: bool,

    /// send messages in behalf of a remote client
    #[argh(option)]
    as_remote_client: Option<usize>,

    /// send keys to server and quit
    #[argh(option, short = 'k')]
    keys: Option<String>,

    /// files to open
    #[argh(positional)]
    files: Vec<String>,
}

fn main() {
    let args: Args = argh::from_env();
    if let Err(e) = application::run(args) {
        eprintln!("{}", e);
    }
}
