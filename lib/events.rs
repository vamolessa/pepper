use std::{error::Error, fmt, ops::Range, str::Chars};

use crate::{
    buffer::BufferHandle,
    buffer_position::BufferRange,
    client::ClientHandle,
    platform::Key,
    serialization::{DeserializationSlice, DeserializeError, Deserializer, Serialize, Serializer},
};

pub struct EditorEventText {
    texts_range: Range<usize>,
}
impl EditorEventText {
    pub fn as_str<'a>(&self, events: &'a EditorEventQueue) -> &'a str {
        &events.read.texts[self.texts_range.clone()]
    }
}

pub enum EditorEvent {
    Idle,
    BufferOpen {
        handle: BufferHandle,
    },
    BufferInsertText {
        handle: BufferHandle,
        range: BufferRange,
        text: EditorEventText,
    },
    BufferDeleteText {
        handle: BufferHandle,
        range: BufferRange,
    },
    BufferSave {
        handle: BufferHandle,
        new_path: bool,
    },
    BufferClose {
        handle: BufferHandle,
    },
}

#[derive(Default)]
struct EventQueue {
    events: Vec<EditorEvent>,
    texts: String,
}

#[derive(Default)]
pub struct EditorEventQueue {
    read: EventQueue,
    write: EventQueue,
}

impl EditorEventQueue {
    pub fn flip(&mut self) {
        self.read.events.clear();
        self.read.texts.clear();
        std::mem::swap(&mut self.read, &mut self.write);
    }

    pub fn enqueue(&mut self, event: EditorEvent) {
        self.write.events.push(event);
    }

    pub fn enqueue_buffer_insert(&mut self, handle: BufferHandle, range: BufferRange, text: &str) {
        let start = self.write.texts.len();
        self.write.texts.push_str(text);
        let text = EditorEventText {
            texts_range: start..self.write.texts.len(),
        };
        self.write.events.push(EditorEvent::BufferInsertText {
            handle,
            range,
            text,
        });
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a EditorEvent> {
        self.read.events.iter()
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
        f.write_fmt(format_args!("{} at index: {}", self.error, self.index))
    }
}
impl Error for KeyParseAllError {}

pub struct KeyParser<'a> {
    raw: &'a str,
    chars: Chars<'a>,
}
impl<'a> KeyParser<'a> {
    pub fn new(raw: &'a str) -> Self {
        Self {
            raw,
            chars: raw.chars(),
        }
    }
}
impl<'a> Iterator for KeyParser<'a> {
    type Item = Result<Key, KeyParseAllError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.chars.as_str().is_empty() {
            return None;
        }
        match parse_key(&mut self.chars) {
            Ok(key) => Some(Ok(key)),
            Err(error) => {
                let parsed_len = self.raw.len() - self.chars.as_str().len();
                let index = self.raw[..parsed_len]
                    .char_indices()
                    .next_back()
                    .unwrap_or((0, '\0'))
                    .0;

                Some(Err(KeyParseAllError { index, error }))
            }
        }
    }
}

fn parse_key(chars: &mut impl Iterator<Item = char>) -> Result<Key, KeyParseError> {
    #[inline]
    fn next(chars: &mut impl Iterator<Item = char>) -> Result<char, KeyParseError> {
        match chars.next() {
            Some(c) => Ok(c),
            None => Err(KeyParseError::UnexpectedEnd),
        }
    }

    #[inline]
    fn consume(chars: &mut impl Iterator<Item = char>, c: char) -> Result<(), KeyParseError> {
        let next = next(chars)?;
        if c == next {
            Ok(())
        } else {
            Err(KeyParseError::InvalidCharacter(next))
        }
    }

    #[inline]
    fn consume_str(chars: &mut impl Iterator<Item = char>, s: &str) -> Result<(), KeyParseError> {
        for c in s.chars() {
            consume(chars, c)?
        }
        Ok(())
    }

    let key = match next(chars)? {
        '<' => match next(chars)? {
            'b' => {
                consume_str(chars, "ackspace>")?;
                Key::Backspace
            }
            's' => {
                consume_str(chars, "pace>")?;
                Key::Char(' ')
            }
            'e' => match next(chars)? {
                'n' => match next(chars)? {
                    't' => {
                        consume_str(chars, "er>")?;
                        Key::Enter
                    }
                    'd' => {
                        consume(chars, '>')?;
                        Key::End
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                's' => {
                    consume_str(chars, "c>")?;
                    Key::Esc
                }
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            'l' => {
                consume(chars, 'e')?;
                match next(chars)? {
                    's' => {
                        consume_str(chars, "s>")?;
                        Key::Char('<')
                    }
                    'f' => {
                        consume_str(chars, "t>")?;
                        Key::Left
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                }
            }
            'g' => {
                consume_str(chars, "reater>")?;
                Key::Char('>')
            }
            'r' => {
                consume_str(chars, "ight>")?;
                Key::Right
            }
            'u' => {
                consume_str(chars, "p>")?;
                Key::Up
            }
            'd' => match next(chars)? {
                'o' => {
                    consume_str(chars, "wn>")?;
                    Key::Down
                }
                'e' => {
                    consume_str(chars, "lete>")?;
                    Key::Delete
                }
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            'h' => {
                consume_str(chars, "ome>")?;
                Key::Home
            }
            'p' => {
                consume_str(chars, "age")?;
                match next(chars)? {
                    'u' => {
                        consume_str(chars, "p>")?;
                        Key::PageUp
                    }
                    'd' => {
                        consume_str(chars, "own>")?;
                        Key::PageDown
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                }
            }
            't' => {
                consume_str(chars, "ab>")?;
                Key::Tab
            }
            'f' => {
                let c = next(chars)?;
                match c.to_digit(10) {
                    Some(d0) => {
                        let c = next(chars)?;
                        match c.to_digit(10) {
                            Some(d1) => {
                                consume(chars, '>')?;
                                let n = d0 * 10 + d1;
                                Key::F(n as _)
                            }
                            None => {
                                if c == '>' {
                                    Key::F(d0 as _)
                                } else {
                                    return Err(KeyParseError::InvalidCharacter(c));
                                }
                            }
                        }
                    }
                    None => return Err(KeyParseError::InvalidCharacter(c)),
                }
            }
            'c' => {
                consume(chars, '-')?;
                let c = next(chars)?;
                let key = if c.is_ascii_alphanumeric() {
                    Key::Ctrl(c)
                } else {
                    return Err(KeyParseError::InvalidCharacter(c));
                };
                consume(chars, '>')?;
                key
            }
            'a' => {
                consume(chars, '-')?;
                let c = next(chars)?;
                let key = if c.is_ascii_alphanumeric() {
                    Key::Alt(c)
                } else {
                    return Err(KeyParseError::InvalidCharacter(c));
                };
                consume(chars, '>')?;
                key
            }
            c => return Err(KeyParseError::InvalidCharacter(c)),
        },
        c @ '>' => return Err(KeyParseError::InvalidCharacter(c)),
        c => Key::Char(c),
    };

    Ok(key)
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Key::None => Ok(()),
            Key::Backspace => f.write_str("<backspace>"),
            Key::Enter => f.write_str("<enter>"),
            Key::Left => f.write_str("<left>"),
            Key::Right => f.write_str("<right>"),
            Key::Up => f.write_str("<up>"),
            Key::Down => f.write_str("<down>"),
            Key::Home => f.write_str("<home>"),
            Key::End => f.write_str("<end>"),
            Key::PageUp => f.write_str("<pageup>"),
            Key::PageDown => f.write_str("<pagedown>"),
            Key::Tab => f.write_str("<tab>"),
            Key::Delete => f.write_str("<delete>"),
            Key::F(n) => f.write_fmt(format_args!("<f{}>", n)),
            Key::Char(' ') => f.write_str("<space>"),
            Key::Char('<') => f.write_str("<less>"),
            Key::Char('>') => f.write_str("<greater>"),
            Key::Char(c) => f.write_fmt(format_args!("{}", c)),
            Key::Ctrl(c) => f.write_fmt(format_args!("<c-{}>", c)),
            Key::Alt(c) => f.write_fmt(format_args!("<a-{}>", c)),
            Key::Esc => f.write_str("<esc>"),
        }
    }
}

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
            let n = Serialize::deserialize(deserializer)?;
            Ok(Key::F(n))
        }
        14 => {
            let c = Serialize::deserialize(deserializer)?;
            Ok(Key::Char(c))
        }
        15 => {
            let c = Serialize::deserialize(deserializer)?;
            Ok(Key::Ctrl(c))
        }
        16 => {
            let c = Serialize::deserialize(deserializer)?;
            Ok(Key::Alt(c))
        }
        17 => Ok(Key::Esc),
        _ => Err(DeserializeError),
    }
}

pub enum ClientEvent<'a> {
    Key(ClientHandle, Key),
    Resize(ClientHandle, u16, u16),
    Command(ClientHandle, &'a str),
}

impl<'de> Serialize<'de> for ClientEvent<'de> {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            ClientEvent::Key(client_handle, key) => {
                0u8.serialize(serializer);
                client_handle.serialize(serializer);
                serialize_key(*key, serializer);
            }
            ClientEvent::Resize(client_handle, width, height) => {
                1u8.serialize(serializer);
                client_handle.serialize(serializer);
                width.serialize(serializer);
                height.serialize(serializer);
            }
            ClientEvent::Command(client_handle, command) => {
                2u8.serialize(serializer);
                client_handle.serialize(serializer);
                command.serialize(serializer);
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
                let handle = Serialize::deserialize(deserializer)?;
                let key = deserialize_key(deserializer)?;
                Ok(ClientEvent::Key(handle, key))
            }
            1 => {
                let handle = Serialize::deserialize(deserializer)?;
                let width = u16::deserialize(deserializer)?;
                let height = u16::deserialize(deserializer)?;
                Ok(ClientEvent::Resize(handle, width, height))
            }
            2 => {
                let handle = Serialize::deserialize(deserializer)?;
                let command = <&str>::deserialize(deserializer)?;
                Ok(ClientEvent::Command(handle, command))
            }
            _ => Err(DeserializeError),
        }
    }
}

pub struct ClientEventIter {
    buf_index: usize,
    read_len: usize,
}
impl ClientEventIter {
    pub fn next<'a>(&mut self, receiver: &'a ClientEventReceiver) -> Option<ClientEvent<'a>> {
        let buf = &receiver.bufs[self.buf_index];
        let slice = &buf[self.read_len..];
        if slice.is_empty() {
            return None;
        }

        let mut deserializer = DeserializationSlice::from_slice(slice);
        match ClientEvent::deserialize(&mut deserializer) {
            Ok(event) => {
                self.read_len = buf.len() - deserializer.as_slice().len();
                Some(event)
            }
            Err(_) => {
                self.read_len = buf.len();
                None
            }
        }
    }

    pub fn finish(&self, receiver: &mut ClientEventReceiver) {
        let buf = &mut receiver.bufs[self.buf_index];
        let rest_len = buf.len() - self.read_len;
        buf.copy_within(self.read_len.., 0);
        buf.truncate(rest_len);
    }
}

#[derive(Default)]
pub struct ClientEventReceiver {
    bufs: Vec<Vec<u8>>,
}

impl ClientEventReceiver {
    pub fn receive_events(&mut self, client_handle: ClientHandle, bytes: &[u8]) -> ClientEventIter {
        let buf_index = client_handle.into_index();
        if buf_index >= self.bufs.len() {
            self.bufs.resize_with(buf_index + 1, Default::default);
        }
        let buf = &mut self.bufs[buf_index];
        buf.extend_from_slice(bytes);
        ClientEventIter {
            buf_index,
            read_len: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_parsing() {
        assert_eq!(
            Key::Backspace,
            parse_key(&mut "<backspace>".chars()).unwrap()
        );
        assert_eq!(Key::Char(' '), parse_key(&mut "<space>".chars()).unwrap());
        assert_eq!(Key::Enter, parse_key(&mut "<enter>".chars()).unwrap());
        assert_eq!(Key::Left, parse_key(&mut "<left>".chars()).unwrap());
        assert_eq!(Key::Right, parse_key(&mut "<right>".chars()).unwrap());
        assert_eq!(Key::Up, parse_key(&mut "<up>".chars()).unwrap());
        assert_eq!(Key::Down, parse_key(&mut "<down>".chars()).unwrap());
        assert_eq!(Key::Home, parse_key(&mut "<home>".chars()).unwrap());
        assert_eq!(Key::End, parse_key(&mut "<end>".chars()).unwrap());
        assert_eq!(Key::PageUp, parse_key(&mut "<pageup>".chars()).unwrap());
        assert_eq!(Key::PageDown, parse_key(&mut "<pagedown>".chars()).unwrap());
        assert_eq!(Key::Tab, parse_key(&mut "<tab>".chars()).unwrap());
        assert_eq!(Key::Delete, parse_key(&mut "<delete>".chars()).unwrap());
        assert_eq!(Key::Esc, parse_key(&mut "<esc>".chars()).unwrap());

        for n in 1..=99 {
            let s = format!("<f{}>", n);
            assert_eq!(Key::F(n as _), parse_key(&mut s.chars()).unwrap());
        }

        assert_eq!(Key::Ctrl('z'), parse_key(&mut "<c-z>".chars()).unwrap());
        assert_eq!(Key::Ctrl('0'), parse_key(&mut "<c-0>".chars()).unwrap());
        assert_eq!(Key::Ctrl('9'), parse_key(&mut "<c-9>".chars()).unwrap());

        assert_eq!(Key::Alt('a'), parse_key(&mut "<a-a>".chars()).unwrap());
        assert_eq!(Key::Alt('z'), parse_key(&mut "<a-z>".chars()).unwrap());
        assert_eq!(Key::Alt('0'), parse_key(&mut "<a-0>".chars()).unwrap());
        assert_eq!(Key::Alt('9'), parse_key(&mut "<a-9>".chars()).unwrap());

        assert_eq!(Key::Char('a'), parse_key(&mut "a".chars()).unwrap());
        assert_eq!(Key::Char('z'), parse_key(&mut "z".chars()).unwrap());
        assert_eq!(Key::Char('0'), parse_key(&mut "0".chars()).unwrap());
        assert_eq!(Key::Char('9'), parse_key(&mut "9".chars()).unwrap());
        assert_eq!(Key::Char('_'), parse_key(&mut "_".chars()).unwrap());
        assert_eq!(Key::Char('<'), parse_key(&mut "<less>".chars()).unwrap());
        assert_eq!(Key::Char('>'), parse_key(&mut "<greater>".chars()).unwrap());
        assert_eq!(Key::Char('\\'), parse_key(&mut "\\".chars()).unwrap());
    }

    #[test]
    fn key_serialization() {
        use crate::serialization::{DeserializationSlice, SerializationBuf};

        fn assert_key_serialization(key: Key) {
            let mut buf = SerializationBuf::default();
            let _ = serialize_key(key, &mut buf);
            let slice = buf.as_slice();
            let mut deserializer = DeserializationSlice::from_slice(slice);
            assert!(!deserializer.as_slice().is_empty());
            match deserialize_key(&mut deserializer) {
                Ok(k) => assert_eq!(key, k),
                Err(_) => assert!(false),
            }
        }

        assert_key_serialization(Key::None);
        assert_key_serialization(Key::Backspace);
        assert_key_serialization(Key::Enter);
        assert_key_serialization(Key::Left);
        assert_key_serialization(Key::Right);
        assert_key_serialization(Key::Up);
        assert_key_serialization(Key::Down);
        assert_key_serialization(Key::Home);
        assert_key_serialization(Key::End);
        assert_key_serialization(Key::PageUp);
        assert_key_serialization(Key::PageDown);
        assert_key_serialization(Key::Tab);
        assert_key_serialization(Key::Delete);
        assert_key_serialization(Key::F(0));
        assert_key_serialization(Key::F(9));
        assert_key_serialization(Key::F(12));
        assert_key_serialization(Key::F(99));
        assert_key_serialization(Key::Char('a'));
        assert_key_serialization(Key::Char('z'));
        assert_key_serialization(Key::Char('A'));
        assert_key_serialization(Key::Char('Z'));
        assert_key_serialization(Key::Char('0'));
        assert_key_serialization(Key::Char('9'));
        assert_key_serialization(Key::Char('$'));
        assert_key_serialization(Key::Ctrl('a'));
        assert_key_serialization(Key::Ctrl('z'));
        assert_key_serialization(Key::Ctrl('A'));
        assert_key_serialization(Key::Ctrl('Z'));
        assert_key_serialization(Key::Ctrl('0'));
        assert_key_serialization(Key::Ctrl('9'));
        assert_key_serialization(Key::Ctrl('$'));
        assert_key_serialization(Key::Alt('a'));
        assert_key_serialization(Key::Alt('z'));
        assert_key_serialization(Key::Alt('A'));
        assert_key_serialization(Key::Alt('Z'));
        assert_key_serialization(Key::Alt('0'));
        assert_key_serialization(Key::Alt('9'));
        assert_key_serialization(Key::Alt('$'));
        assert_key_serialization(Key::Esc);
    }
}
