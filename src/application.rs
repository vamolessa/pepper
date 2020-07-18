use std::{convert::From, env, fs, io};

use uds_windows::UnixStream;

use argh::FromArgs;
use futures::{
    future::FutureExt,
    pin_mut, select_biased,
    stream::{FusedStream, FuturesUnordered, StreamExt},
};

use crate::{
    client::Client,
    connection::{ClientListener, ConnectionWithClientCollection, TargetClient},
    editor::{Editor, EditorLoop, EditorOperationSender},
    event::Event,
    mode::Mode,
};

#[derive(Debug)]
pub enum ApplicationError<UiError> {
    IO(io::Error),
    UI(UiError),
}

impl<UiError> From<io::Error> for ApplicationError<UiError> {
    fn from(error: io::Error) -> Self {
        ApplicationError::IO(error)
    }
}

#[derive(FromArgs)]
/// pepper editor
struct Args {
    //#[argh(option, short = 's')]
///// session to connect to
//session: Option<String>,
}

pub trait UI {
    type Error;

    fn init(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resize(&mut self, _width: u16, _height: u16) -> Result<(), Self::Error> {
        Ok(())
    }

    fn draw(&mut self, client: &Client, error: Option<String>) -> Result<(), Self::Error>;

    fn shutdown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn bind_keys(editor: &mut Editor) {
    editor
        .keymaps
        .parse_map(Mode::Normal.discriminant(), "qq", ":quit<c-m>")
        .unwrap();
    editor
        .keymaps
        .parse_map(Mode::Normal.discriminant(), "edit", "i")
        .unwrap();
    editor
        .keymaps
        .parse_map(Mode::Normal.discriminant(), "dl", "vld")
        .unwrap();
    editor
        .keymaps
        .parse_map(Mode::Normal.discriminant(), "dh", "vvhd")
        .unwrap();
    editor
        .keymaps
        .parse_map(Mode::Normal.discriminant(), "<c-f>", ":find-command<c-m>")
        .unwrap();
}

fn send_operations(operations: &mut EditorOperationSender, local_client: &mut Client) {
    for (target_client, operation, content) in operations.drain() {
        match target_client {
            TargetClient::All => {
                local_client.on_editor_operation(operation, content);
            }
            TargetClient::Local => local_client.on_editor_operation(operation, content),
            _ => (),
        }
    }
}

pub async fn run<E, I>(event_stream: E, ui: I) -> Result<(), ApplicationError<I::Error>>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    //let args: Args = argh::from_env();

    let session_socket_path = env::current_dir()?.join("session_socket");
    if let Ok(_stream) = UnixStream::connect(&session_socket_path) {
        run_client(event_stream, ui).await?;
    } else if let Ok(listener) = ClientListener::listen(&session_socket_path) {
        run_server_with_client(event_stream, ui, listener).await?;
        fs::remove_file(session_socket_path)?;
    } else if let Ok(()) = fs::remove_file(&session_socket_path) {
        let listener = ClientListener::listen(&session_socket_path)?;
        run_server_with_client(event_stream, ui, listener).await?;
        fs::remove_file(session_socket_path)?;
    }

    Ok(())
}

async fn run_server_with_client<E, I>(
    event_stream: E,
    mut ui: I,
    listener: ClientListener,
) -> Result<(), ApplicationError<I::Error>>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    ui.init().map_err(|e| ApplicationError::UI(e))?;

    let mut local_client = Client::new();
    let mut editor = Editor::new();
    bind_keys(&mut editor);

    let mut client_connections = ConnectionWithClientCollection::default();
    //let mut clients_key_futures = FuturesUnordered::new();

    let mut editor_operations = EditorOperationSender::new();

    let listen_future = listener.accept().fuse();
    pin_mut!(event_stream, listen_future);
    loop {
        let mut error = None;

        select_biased! {
            event = event_stream.select_next_some() => {
                match event {
                    Event::Key(key) => {
                        match editor.on_key(key, TargetClient::Local, &mut editor_operations) {
                            EditorLoop::Quit => break,
                            EditorLoop::Continue => (),
                            EditorLoop::Error(e) => error = Some(e),
                        }
                        send_operations(&mut editor_operations, &mut local_client);
                    },
                    Event::Resize(w, h) => ui.resize(w, h).map_err(|e| ApplicationError::UI(e))?,
                    _ => break,
                }
            },
            connection = listen_future => {
                listen_future.set(listener.accept().fuse());
                client_connections.add(connection?);
            },
        }

        ui.draw(&local_client, error)
            .map_err(|e| ApplicationError::UI(e))?;
    }

    ui.shutdown().map_err(|e| ApplicationError::UI(e))?;
    Ok(())
}

async fn run_client<E, I>(_event_stream: E, _ui: I) -> Result<(), ApplicationError<I::Error>>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    Ok(())
}
