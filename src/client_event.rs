use std::{error::Error, fmt, str::Chars};

use crate::{
    event_manager::ConnectionEvent,
    serialization::{
        DeserializationSlice, DeserializeError, Deserializer, SerializationBuf, Serialize,
        Serializer,
    },
};

#[derive(Debug, Clone, Copy)]
pub enum LocalEvent {
    None,
    Key(Key),
    Resize(u16, u16),
    Connection(ConnectionEvent),
}

#[derive(Debug)]
pub enum ClientEvent<'a> {
    AsFocusedClient,
    AsClient(usize),
    OpenFile(&'a str),
    Key(Key),
    Resize(u16, u16),
}

impl<'de> Serialize<'de> for ClientEvent<'de> {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            ClientEvent::AsFocusedClient => 0u8.serialize(serializer),
            ClientEvent::AsClient(index) => {
                1u8.serialize(serializer);
                let index = *index as u32;
                index.serialize(serializer);
            }
            ClientEvent::OpenFile(path) => {
                2u8.serialize(serializer);
                path.serialize(serializer);
            }
            ClientEvent::Key(key) => {
                3u8.serialize(serializer);
                key.serialize(serializer);
            }
            ClientEvent::Resize(width, height) => {
                4u8.serialize(serializer);
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
            0 => Ok(ClientEvent::AsFocusedClient),
            1 => {
                let index = u32::deserialize(deserializer)?;
                Ok(ClientEvent::AsClient(index as _))
            }
            2 => {
                let path = <&str>::deserialize(deserializer)?;
                Ok(ClientEvent::OpenFile(path))
            }
            3 => {
                let key = Key::deserialize(deserializer)?;
                Ok(ClientEvent::Key(key))
            }
            4 => {
                let width = u16::deserialize(deserializer)?;
                let height = u16::deserialize(deserializer)?;
                Ok(ClientEvent::Resize(width, height))
            }
            _ => Err(DeserializeError),
        }
    }
}

#[derive(Debug)]
pub enum KeyParseError {
    UnexpectedEnd,
    InvalidCharacter(char),
}
impl fmt::Display for KeyParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::UnexpectedEnd => write!(f, "could not finish parsing key"),
            Self::InvalidCharacter(c) => write!(f, "invalid character {}", c),
        }
    }
}
impl Error for KeyParseError {}

#[derive(Debug)]
pub struct KeyParseAllError {
    pub index: usize,
    pub error: KeyParseError,
}
impl fmt::Display for KeyParseAllError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.error.fmt(f)?;
        f.write_fmt(format_args!(" at index: {}", self.index))?;
        Ok(())
    }
}
impl Error for KeyParseAllError {}

pub struct KeyParser<'a> {
    len: usize,
    chars: Chars<'a>,
}
impl<'a> Iterator for KeyParser<'a> {
    type Item = Result<Key, KeyParseAllError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.chars.as_str().is_empty() {
            return None;
        }
        match Key::parse(&mut self.chars) {
            Ok(key) => Some(Ok(key)),
            Err(error) => Some(Err(KeyParseAllError {
                index: self.len - self.chars.as_str().len(),
                error,
            })),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    None,
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    Delete,
    F(u8),
    Char(char),
    Ctrl(char),
    Alt(char),
    Esc,
}

impl<'de> Serialize<'de> for Key {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
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

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
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
}

impl Key {
    pub fn parse_all<'a>(raw: &'a str) -> KeyParser<'a> {
        KeyParser {
            len: raw.len(),
            chars: raw.chars(),
        }
    }

    pub fn parse(chars: &mut impl Iterator<Item = char>) -> Result<Self, KeyParseError> {
        macro_rules! next {
            () => {
                match chars.next() {
                    Some(element) => element,
                    None => return Err(KeyParseError::UnexpectedEnd),
                }
            };
        }

        macro_rules! consume {
            ($character:expr) => {
                let c = next!();
                if c != $character {
                    return Err(KeyParseError::InvalidCharacter(c));
                }
            };
        }

        macro_rules! consume_str {
            ($str:expr) => {
                for c in $str.chars() {
                    consume!(c);
                }
            };
        }

        let key = match next!() {
            '\\' => match next!() {
                '\\' => Key::Char('\\'),
                '<' => Key::Char('<'),
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            '<' => match next!() {
                'b' => {
                    consume_str!("ackspace>");
                    Key::Backspace
                }
                's' => {
                    consume_str!("pace>");
                    Key::Char(' ')
                }
                'e' => match next!() {
                    'n' => match next!() {
                        't' => {
                            consume_str!("er>");
                            Key::Enter
                        }
                        'd' => {
                            consume!('>');
                            Key::End
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    },
                    's' => {
                        consume_str!("c>");
                        Key::Esc
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'l' => {
                    consume_str!("eft>");
                    Key::Left
                }
                'r' => {
                    consume_str!("ight>");
                    Key::Right
                }
                'u' => {
                    consume_str!("p>");
                    Key::Up
                }
                'd' => match next!() {
                    'o' => {
                        consume_str!("wn>");
                        Key::Down
                    }
                    'e' => {
                        consume_str!("lete>");
                        Key::Delete
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'h' => {
                    consume_str!("ome>");
                    Key::Home
                }
                'p' => {
                    consume_str!("age");
                    match next!() {
                        'u' => {
                            consume_str!("p>");
                            Key::PageUp
                        }
                        'd' => {
                            consume_str!("own>");
                            Key::PageDown
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    }
                }
                't' => {
                    consume_str!("ab>");
                    Key::Tab
                }
                'f' => {
                    let n = match next!() {
                        '1' => match next!() {
                            '>' => 1,
                            '0' => {
                                consume!('>');
                                10
                            }
                            '1' => {
                                consume!('>');
                                11
                            }
                            '2' => {
                                consume!('>');
                                12
                            }
                            c => return Err(KeyParseError::InvalidCharacter(c)),
                        },
                        c => {
                            consume!('>');
                            match c.to_digit(10) {
                                Some(n) => n,
                                None => return Err(KeyParseError::InvalidCharacter(c)),
                            }
                        }
                    };
                    Key::F(n as _)
                }
                'c' => {
                    consume!('-');
                    let c = next!();
                    let key = if c.is_ascii_alphanumeric() {
                        Key::Ctrl(c)
                    } else {
                        return Err(KeyParseError::InvalidCharacter(c));
                    };
                    consume!('>');
                    key
                }
                'a' => {
                    consume!('-');
                    let c = next!();
                    let key = if c.is_ascii_alphanumeric() {
                        Key::Alt(c)
                    } else {
                        return Err(KeyParseError::InvalidCharacter(c));
                    };
                    consume!('>');
                    key
                }
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            c => {
                if c.is_ascii() {
                    Key::Char(c)
                } else {
                    return Err(KeyParseError::InvalidCharacter(c));
                }
            }
        };

        Ok(key)
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

#[derive(Debug)]
pub enum ClientEventDeserializeResult<'a> {
    Some(ClientEvent<'a>),
    None,
    Error,
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
            Err(_) => ClientEventDeserializeResult::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key() {
        assert_eq!(
            Key::Backspace,
            Key::parse(&mut "<backspace>".chars()).unwrap()
        );
        assert_eq!(Key::Char(' '), Key::parse(&mut "<space>".chars()).unwrap());
        assert_eq!(Key::Enter, Key::parse(&mut "<enter>".chars()).unwrap());
        assert_eq!(Key::Left, Key::parse(&mut "<left>".chars()).unwrap());
        assert_eq!(Key::Right, Key::parse(&mut "<right>".chars()).unwrap());
        assert_eq!(Key::Up, Key::parse(&mut "<up>".chars()).unwrap());
        assert_eq!(Key::Down, Key::parse(&mut "<down>".chars()).unwrap());
        assert_eq!(Key::Home, Key::parse(&mut "<home>".chars()).unwrap());
        assert_eq!(Key::End, Key::parse(&mut "<end>".chars()).unwrap());
        assert_eq!(Key::PageUp, Key::parse(&mut "<pageup>".chars()).unwrap());
        assert_eq!(
            Key::PageDown,
            Key::parse(&mut "<pagedown>".chars()).unwrap()
        );
        assert_eq!(Key::Tab, Key::parse(&mut "<tab>".chars()).unwrap());
        assert_eq!(Key::Delete, Key::parse(&mut "<delete>".chars()).unwrap());
        assert_eq!(Key::Esc, Key::parse(&mut "<esc>".chars()).unwrap());

        for n in 1..=12 {
            let s = format!("<f{}>", n);
            assert_eq!(Key::F(n as _), Key::parse(&mut s.chars()).unwrap());
        }

        assert_eq!(Key::Ctrl('z'), Key::parse(&mut "<c-z>".chars()).unwrap());
        assert_eq!(Key::Ctrl('0'), Key::parse(&mut "<c-0>".chars()).unwrap());
        assert_eq!(Key::Ctrl('9'), Key::parse(&mut "<c-9>".chars()).unwrap());

        assert_eq!(Key::Alt('a'), Key::parse(&mut "<a-a>".chars()).unwrap());
        assert_eq!(Key::Alt('z'), Key::parse(&mut "<a-z>".chars()).unwrap());
        assert_eq!(Key::Alt('0'), Key::parse(&mut "<a-0>".chars()).unwrap());
        assert_eq!(Key::Alt('9'), Key::parse(&mut "<a-9>".chars()).unwrap());

        assert_eq!(Key::Char('a'), Key::parse(&mut "a".chars()).unwrap());
        assert_eq!(Key::Char('z'), Key::parse(&mut "z".chars()).unwrap());
        assert_eq!(Key::Char('0'), Key::parse(&mut "0".chars()).unwrap());
        assert_eq!(Key::Char('9'), Key::parse(&mut "9".chars()).unwrap());
        assert_eq!(Key::Char('_'), Key::parse(&mut "_".chars()).unwrap());
        assert_eq!(Key::Char('<'), Key::parse(&mut "\\<".chars()).unwrap());
        assert_eq!(Key::Char('\\'), Key::parse(&mut "\\\\".chars()).unwrap());
    }

    #[test]
    fn key_serialization() {
        macro_rules! assert_key_serialization {
            ($key:expr) => {
                let mut serializer = ClientEventSerializer::default();
                serializer.serialize(ClientEvent::Key($key));
                let slice = serializer.bytes();
                let mut deserializer = ClientEventDeserializer::from_slice(slice);
                if let ClientEventDeserializeResult::Some(ClientEvent::Key(key)) =
                    deserializer.deserialize_next()
                {
                    assert_eq!($key, key);
                } else {
                    assert!(false);
                }
            };
        }

        assert_key_serialization!(Key::None);
        assert_key_serialization!(Key::Backspace);
        assert_key_serialization!(Key::Enter);
        assert_key_serialization!(Key::Left);
        assert_key_serialization!(Key::Right);
        assert_key_serialization!(Key::Up);
        assert_key_serialization!(Key::Down);
        assert_key_serialization!(Key::Home);
        assert_key_serialization!(Key::End);
        assert_key_serialization!(Key::PageUp);
        assert_key_serialization!(Key::PageDown);
        assert_key_serialization!(Key::Tab);
        assert_key_serialization!(Key::Delete);
        assert_key_serialization!(Key::F(0));
        assert_key_serialization!(Key::F(9));
        assert_key_serialization!(Key::F(12));
        assert_key_serialization!(Key::Char('a'));
        assert_key_serialization!(Key::Char('z'));
        assert_key_serialization!(Key::Char('A'));
        assert_key_serialization!(Key::Char('Z'));
        assert_key_serialization!(Key::Char('0'));
        assert_key_serialization!(Key::Char('9'));
        assert_key_serialization!(Key::Char('$'));
        assert_key_serialization!(Key::Ctrl('a'));
        assert_key_serialization!(Key::Ctrl('z'));
        assert_key_serialization!(Key::Ctrl('A'));
        assert_key_serialization!(Key::Ctrl('Z'));
        assert_key_serialization!(Key::Ctrl('0'));
        assert_key_serialization!(Key::Ctrl('9'));
        assert_key_serialization!(Key::Ctrl('$'));
        assert_key_serialization!(Key::Alt('a'));
        assert_key_serialization!(Key::Alt('z'));
        assert_key_serialization!(Key::Alt('A'));
        assert_key_serialization!(Key::Alt('Z'));
        assert_key_serialization!(Key::Alt('0'));
        assert_key_serialization!(Key::Alt('9'));
        assert_key_serialization!(Key::Alt('$'));
        assert_key_serialization!(Key::Esc);
    }
}
