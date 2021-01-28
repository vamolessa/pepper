use crate::platform::Key;

use crate::{
    client::TargetClient,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
    ui::UiKind,
};

fn serialize_key<S>(key: Key, serializer: &mut S)
where
    S: Serializer,
{
    match key {
        Key::None => 0u8.serialize(serializer),
        Key::Backspace => 1u8.serialize(serializer),
        Key::Enter => 2u8.serialize(serializer),
        Key::Left => 3u8.serialize(serializer),
        Key::Right => 4u8.serialize(serializer),
        Key::Up => 5u8.serialize(serializer),
        Key::Down => 6u8.serialize(serializer),
        Key::Home => 7u8.serialize(serializer),
        Key::End => 8u8.serialize(serializer),
        Key::PageUp => 9u8.serialize(serializer),
        Key::PageDown => 10u8.serialize(serializer),
        Key::Tab => 11u8.serialize(serializer),
        Key::Delete => 12u8.serialize(serializer),
        Key::F(n) => {
            13u8.serialize(serializer);
            n.serialize(serializer);
        }
        Key::Char(c) => {
            14u8.serialize(serializer);
            c.serialize(serializer);
        }
        Key::Ctrl(c) => {
            15u8.serialize(serializer);
            c.serialize(serializer);
        }
        Key::Alt(c) => {
            16u8.serialize(serializer);
            c.serialize(serializer);
        }
        Key::Esc => 17u8.serialize(serializer),
    }
}

fn deserialize_key<'de, D>(deserializer: &mut D) -> Result<Key, DeserializeError>
where
    D: Deserializer<'de>,
{
    let discriminant = u8::deserialize(deserializer)?;
    match discriminant {
        0 => Ok(Key::None),
        1 => Ok(Key::Backspace),
        2 => Ok(Key::Enter),
        3 => Ok(Key::Left),
        4 => Ok(Key::Right),
        5 => Ok(Key::Up),
        6 => Ok(Key::Down),
        7 => Ok(Key::Home),
        8 => Ok(Key::End),
        9 => Ok(Key::PageUp),
        10 => Ok(Key::PageDown),
        11 => Ok(Key::Tab),
        12 => Ok(Key::Delete),
        13 => {
            let n = u8::deserialize(deserializer)?;
            Ok(Key::F(n))
        }
        14 => {
            let c = char::deserialize(deserializer)?;
            Ok(Key::Char(c))
        }
        15 => {
            let c = char::deserialize(deserializer)?;
            Ok(Key::Ctrl(c))
        }
        16 => {
            let c = char::deserialize(deserializer)?;
            Ok(Key::Alt(c))
        }
        17 => Ok(Key::Esc),
        _ => Err(DeserializeError),
    }
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
                serialize_key(*key, serializer);
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
                let key = deserialize_key(deserializer)?;
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
