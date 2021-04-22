mod macros;

pub mod application;
pub mod buffer;
pub mod buffer_position;
pub mod buffer_view;
pub mod client;
pub mod command;
pub mod config;
pub mod cursor;
pub mod editor;
pub mod editor_utils;
pub mod events;
pub mod glob;
pub mod history;
pub mod json;
pub mod keymap;
pub mod lsp;
pub mod mode;
pub mod navigation_history;
pub mod pattern;
pub mod picker;
pub mod platform;
pub mod register;
pub mod serialization;
pub mod syntax;
pub mod theme;
pub mod ui;
pub mod word_database;

use argh::FromArgs;

/*
Usage: pepper [<files...>] [-v] [-c <config>] [-s <session>] [--print-session] [--as-client <as-client>]

Pepper An opinionated modal editor to simplify code editing from the terminal

Options:
  -v, --version     print version and quit
  -c, --config      load config file at path (repeatable)
  -s, --session     session name
  --print-session   print the computed session name and exits
  --as-client       displays no ui and send events on behalf of the client at
                    index
  --help            display usage information
*/

/// Pepper
/// An opinionated modal editor to simplify code editing from the terminal
#[derive(FromArgs)]
pub struct Args {
    /// print version and quit
    #[argh(switch, short = 'v')]
    pub version: bool,

    /// load config file at path (repeatable)
    #[argh(option, short = 'c')]
    pub config: Vec<String>,

    /// session name
    #[argh(option, short = 's')]
    pub session: Option<String>,

    /// print the computed session name and exits
    #[argh(switch)]
    pub print_session: bool,

    /// displays no ui and send events on behalf of the client at index
    #[argh(option)]
    pub as_client: Option<client::ClientHandle>,

    /// open files at paths
    /// you can append ':<line-number>' to a path to open it at that line
    #[argh(positional)]
    pub files: Vec<String>,
}

impl Args {
    pub fn parse() -> Option<Self> {
        let args: Args = argh::from_env();
        if args.version {
            let name = env!("CARGO_PKG_NAME");
            let version = env!("CARGO_PKG_VERSION");
            println!("{} version {}", name, version);
            return None;
        }

        if let Some(ref session) = args.session {
            if !session.chars().all(char::is_alphanumeric) {
                panic!(
                    "invalid session name '{}'. it can only contain alphanumeric characters",
                    session
                );
            }
        }

        Some(args)
    }
}