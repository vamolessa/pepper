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
mod editor_event;
mod event_manager;
mod glob;
mod history;
mod json;
mod keymap;
mod lsp;
mod mode;
mod navigation_history;
mod pattern;
mod picker;
mod platform;
mod register;
mod script;
mod script_bindings;
mod serialization;
mod syntax;
mod task;
mod theme;
mod ui;
mod word_database;

use argh::FromArgs;

/// Pepper
/// An opinionated modal editor to simplify code editing from the terminal
#[derive(FromArgs)]
pub struct Args {
    /// print version and quit
    #[argh(switch, short = 'v')]
    version: bool,

    /// load config file at path (repeatable)
    #[argh(option, short = 'c')]
    config: Vec<std::path::PathBuf>,

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
    if false {
        let args: Args = argh::from_env();
        if args.version {
            let name = env!("CARGO_PKG_NAME");
            let version = env!("CARGO_PKG_VERSION");
            println!("{} version {}", name, version);
        } else if let Err(e) = application::run(args) {
            eprintln!("{}", e);
        }
    }

    //platform::run();

    /*
    let args: Args = argh::from_env();
    if args.version {
        let name = env!("CARGO_PKG_NAME");
        let version = env!("CARGO_PKG_VERSION");
        println!("{} version {}", name, version);
    } else {
        if false {
            if let Err(e) = application::run(args) {
                eprintln!("{}", e);
            }
            return;
        }

        //platform::run();
    }
    */
}
