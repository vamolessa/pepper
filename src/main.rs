#![recursion_limit="256"]

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
mod event;
mod history;
mod keymap;
mod mode;
mod theme;
mod tui;

fn main() {
    ctrlc::set_handler(|| {}).unwrap();

    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let ui = tui::Tui::new(stdout);

    smol::run(application::run_server_with_client(tui::event_stream(), ui)).unwrap();
    //smol::run(application::run_client(tui::event_stream(), ui)).unwrap();
}
