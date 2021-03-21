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
