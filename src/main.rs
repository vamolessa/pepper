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
mod register;
mod script;
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

    /// load config file at path (repeatable)
    #[argh(option, short = 'c')]
    config: Vec<std::path::PathBuf>,

    /// adds an extra script module search path (repeatable)
    #[argh(option)]
    module_search_path: Vec<std::path::PathBuf>,

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
    use ui::Ui;
    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let mut ui = ui::tui::Tui::new(stdout);

    let mut lsp = lsp::LspClientCollection::default();
    let server_command = std::process::Command::new("rust-analyzer");
    let (event_sender, event_receiver) = std::sync::mpsc::channel();
    ui.run_event_loop_in_background(event_sender.clone());
    let handle = lsp.spawn(server_command, event_sender).unwrap();
    let client = lsp.get(handle).unwrap();
    client.initialize().unwrap();

    let mut buffers = buffer::BufferCollection::default();
    let mut buffer_views = buffer_view::BufferViewCollection::default();
    let mut status_message = editor::StatusMessage::new();
    let mut ctx = lsp::LspClientContext {
        buffers: &mut buffers,
        buffer_views: &mut buffer_views,
        status_message: &mut status_message,
    };

    for event in event_receiver.iter() {
        match event {
            client_event::LocalEvent::Lsp(handle, event) => {
                lsp.on_server_event(&mut ctx, handle, event).unwrap();
            }
            client_event::LocalEvent::Key(client_event::Key::Char('q')) => break,
            _ => (),
        }
    }
    return;

    let args: Args = argh::from_env();
    if args.version {
        let name = env!("CARGO_PKG_NAME");
        let version = env!("CARGO_PKG_VERSION");
        println!("{} version {}", name, version);
    } else if let Err(e) = application::run(args) {
        eprintln!("{}", e);
    }
}
