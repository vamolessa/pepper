use futures::{
    pin_mut, select_biased,
    stream::{FusedStream, StreamExt},
};

use crate::{
    client::Client,
    connection::TargetClient,
    editor::{Editor, EditorLoop, EditorOperationSender},
    event::Event,
    mode::Mode,
};

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

pub async fn run_server_with_client<E, I>(event_stream: E, mut ui: I) -> Result<(), ()>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    if ui.init().is_err() {
        return Err(());
    }

    let mut local_client = Client::new();
    let mut editor = Editor::new();
    bind_keys(&mut editor);

    let mut editor_operations = EditorOperationSender::new();

    pin_mut!(event_stream);
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
                    Event::Resize(w, h) => if ui.resize(w, h).is_err() {
                        return Err(());
                    }
                    _ => break,
                }
            },
        }

        if ui.draw(&local_client, error).is_err() {
            return Err(());
        }
    }

    if ui.shutdown().is_err() {
        return Err(());
    }

    Ok(())
}
