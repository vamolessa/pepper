use argh::FromArgs;

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

#[derive(FromArgs)]
/// pepper editor
struct Args {
    #[argh(option, short = 's')]
    /// session to connect to
    session: Option<String>,
}

fn main() {
    ctrlc::set_handler(|| {}).unwrap();

    let args: Args = argh::from_env();

    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let ui = tui::Tui::new(stdout);

    smol::run(application::run_server_with_client(tui::event_stream(), ui)).unwrap();
    //smol::run(application::run_client(tui::event_stream(), ui)).unwrap();
}
