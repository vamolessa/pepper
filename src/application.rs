use futures::{
    pin_mut, select_biased,
    stream::{FusedStream, StreamExt},
};

use crate::{
    client::Client,
    editor::{Editor, EditorPollResult},
    event::Event,
};

pub trait UI {
    type Error;

    fn init(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn draw(
        &mut self,
        client: &Client,
        width: u16,
        height: u16,
        error: Option<String>,
    ) -> Result<(), Self::Error>;

    fn shutdown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub async fn run_server_with_client<E, I>(event_stream: E, mut ui: I) -> Result<(), ()>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    if let Err(_) = ui.init() {
        return Err(());
    }

    let mut local_client = Client::new();
    let mut editor = Editor::new();

    let mut available_width = 0;
    let mut available_height = 0;

    pin_mut!(event_stream);
    loop {
        let mut error = None;

        select_biased! {
            event = event_stream.select_next_some() => {
                match event {
                    Event::Key(key) => {
                        match client_editor .on_key(key){
                            EditorPollResult::Pending => (),
                            EditorPollResult::Quit => break,
                            EditorPollResult::Error(e) => error = Some(e),
                        }
                        //local_client.on_editor_operation();
                    },
                    Event::Resize(w, h) => {
                        available_width = w;
                        available_height = h;
                    }
                    _ => break,
                }
            },
        }

        if let Err(_) = ui.draw(&local_client, available_width, available_height, error) {
            return Err(());
        }
    }

    if let Err(_) = ui.shutdown() {
        return Err(());
    }

    Ok(())
}
