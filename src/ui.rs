use std::{error::Error, io, sync::mpsc, thread};

use crate::{
    client::{Client, TargetClient},
    client_event::{Key, LocalEvent},
    editor::Editor,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
};

pub mod none_ui;
pub mod tui;

pub type UiResult<T> = Result<T, Box<dyn 'static + Error>>;

#[derive(Debug)]
pub enum UiKind {
    None,
    Tui { status_bar_buf: String },
}

impl UiKind {
    pub fn render(
        &mut self,
        editor: &Editor,
        client: &Client,
        target_client: TargetClient,
        buffer: &mut Vec<u8>,
    ) -> UiResult<()> {
        buffer.clear();
        match self {
            Self::None => Ok(()),
            Self::Tui {
                ref mut status_bar_buf,
            } => tui::render(editor, client, target_client, buffer, status_bar_buf),
        }
    }
}

impl Default for UiKind {
    fn default() -> Self {
        Self::Tui {
            status_bar_buf: String::new(),
        }
    }
}

impl<'de> Serialize<'de> for UiKind {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            UiKind::None => 0u8.serialize(serializer),
            UiKind::Tui { .. } => 1u8.serialize(serializer),
        }
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        let discriminant = u8::deserialize(deserializer)?;
        match discriminant {
            0 => Ok(UiKind::None),
            1 => Ok(UiKind::Tui {
                status_bar_buf: String::new(),
            }),
            _ => Err(DeserializeError),
        }
    }
}

pub trait Ui {
    fn run_event_loop_in_background(
        &mut self,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> thread::JoinHandle<()>;

    fn init(&mut self) -> UiResult<()> {
        Ok(())
    }

    fn display(&mut self, buffer: &[u8]) -> UiResult<()>;

    fn shutdown(&mut self) -> UiResult<()> {
        Ok(())
    }
}

pub fn read_keys_from_stdin(event_sender: mpsc::Sender<LocalEvent>) {
    use io::BufRead;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut line = String::new();

    'main_loop: loop {
        if stdin.read_line(&mut line).is_err() || line.is_empty() {
            break;
        }

        for key in Key::parse_all(&line) {
            match key {
                Ok(key) => {
                    if event_sender.send(LocalEvent::Key(key)).is_err() {
                        break 'main_loop;
                    }
                }
                Err(_) => break,
            }
        }

        line.clear();
    }

    let _ = event_sender.send(LocalEvent::EndOfInput);
}
