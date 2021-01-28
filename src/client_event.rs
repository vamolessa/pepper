use crate::platform::Key;

use crate::{
    client::TargetClient,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
    ui::UiKind,
};

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
