use crate::platform::Key;

use crate::{
    client::TargetClient,
    event_manager::ConnectionEvent,
    lsp::{LspClientHandle, LspServerEvent},
    serialization::{
        DeserializationSlice, DeserializeError, Deserializer, SerializationBuf, Serialize,
        Serializer,
    },
    task::{TaskHandle, TaskResult},
    ui::UiKind,
};

pub enum LocalEvent {
    None,
    EndOfInput,
    Idle,
    Repaint,
    Key(Key),
    Resize(u16, u16),
    Connection(ConnectionEvent),
    TaskEvent(TargetClient, TaskHandle, TaskResult),
    Lsp(LspClientHandle, LspServerEvent),
}

pub enum ClientEvent<'a> {
    Ui(UiKind),
    AsFocusedClient,
    AsClient(TargetClient),
    OpenBuffer(&'a str),
    Key(Key),
    Resize(u16, u16),
}

impl<'de> Serialize<'de> for ClientEvent<'de> {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            ClientEvent::Ui(ui) => {
                0u8.serialize(serializer);
                ui.serialize(serializer);
            }
            ClientEvent::AsFocusedClient => 1u8.serialize(serializer),
            ClientEvent::AsClient(target_client) => {
                2u8.serialize(serializer);
                target_client.serialize(serializer);
            }
            ClientEvent::OpenBuffer(path) => {
                3u8.serialize(serializer);
                path.serialize(serializer);
            }
            ClientEvent::Key(key) => {
                4u8.serialize(serializer);
                key.serialize(serializer);
            }
            ClientEvent::Resize(width, height) => {
                5u8.serialize(serializer);
                width.serialize(serializer);
                height.serialize(serializer);
            }
        }
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        let discriminant = u8::deserialize(deserializer)?;
        match discriminant {
            0 => {
                let ui = UiKind::deserialize(deserializer)?;
                Ok(ClientEvent::Ui(ui))
            }
            1 => Ok(ClientEvent::AsFocusedClient),
            2 => {
                let target_client = TargetClient::deserialize(deserializer)?;
                Ok(ClientEvent::AsClient(target_client))
            }
            3 => {
                let path = <&str>::deserialize(deserializer)?;
                Ok(ClientEvent::OpenBuffer(path))
            }
            4 => {
                let key = Key::deserialize(deserializer)?;
                Ok(ClientEvent::Key(key))
            }
            5 => {
                let width = u16::deserialize(deserializer)?;
                let height = u16::deserialize(deserializer)?;
                Ok(ClientEvent::Resize(width, height))
            }
            _ => Err(DeserializeError),
        }
    }
}

#[derive(Default)]
pub struct ClientEventSerializer(SerializationBuf);

impl ClientEventSerializer {
    pub fn serialize(&mut self, event: ClientEvent) {
        let _ = event.serialize(&mut self.0);
    }

    pub fn bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

pub enum ClientEventDeserializeResult<'a> {
    Some(ClientEvent<'a>),
    None,
    Error(&'a [u8]),
}

pub struct ClientEventDeserializer<'a>(DeserializationSlice<'a>);

impl<'a> ClientEventDeserializer<'a> {
    pub fn from_slice(slice: &'a [u8]) -> Self {
        Self(DeserializationSlice::from_slice(slice))
    }

    pub fn deserialize_next(&mut self) -> ClientEventDeserializeResult {
        if self.0.as_slice().is_empty() {
            return ClientEventDeserializeResult::None;
        }

        match ClientEvent::deserialize(&mut self.0) {
            Ok(event) => ClientEventDeserializeResult::Some(event),
            Err(_) => ClientEventDeserializeResult::Error(self.0.as_slice()),
        }
    }
}
