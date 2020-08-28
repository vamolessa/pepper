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

fn main() {
    if let Err(e) = application::run() {
        eprintln!("{}", e);
    }
}
