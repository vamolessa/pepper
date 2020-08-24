mod application;
mod buffer;
mod buffer_position;
mod buffer_view;
mod client;
mod command;
mod config;
mod connection;
mod cursor;
mod editor;
mod editor_operation;
mod client_event;
mod event_manager;
mod history;
mod keymap;
mod mode;
mod pattern;
mod select;
mod serialization;
mod syntax;
mod theme;
mod tui;

fn main() {
    if let Err(e) = ctrlc::set_handler(|| {}) {
        eprintln!("could not set ctrl-c handler: {:?}", e);
        return;
    }

    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let ui = tui::Tui::new(stdout);

    if let Err(e) = application::run(ui) {
        eprintln!("{:?}", e);
    }
}
