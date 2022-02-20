use std::{error::Error, fmt, str::Chars};

use crate::{
    buffer::BufferHandle,
    buffer_position::BufferRange,
    buffer_view::BufferViewHandle,
    client::ClientHandle,
    cursor::Cursor,
    platform::AnsiKey,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
};

#[derive(Clone, Copy)]
pub struct EditorEventText {
    from: u32,
    to: u32,
}
impl EditorEventText {
    pub fn as_str<'a>(&self, events: &'a EditorEventQueue) -> &'a str {
        &events.read.texts[self.from as usize..self.to as usize]
    }
}

#[derive(Clone, Copy)]
pub struct EditorEventCursors {
    from: u32,
    to: u32,
}
impl EditorEventCursors {
    pub fn as_cursors<'a>(&self, events: &'a EditorEventQueue) -> &'a [Cursor] {
        &events.read.cursors[self.from as usize..self.to as usize]
    }
}

pub enum EditorEvent {
    Idle,
    BufferRead {
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
    BufferWrite {
        handle: BufferHandle,
        new_path: bool,
    },
    BufferClose {
        handle: BufferHandle,
    },
    FixCursors {
        handle: BufferViewHandle,
        cursors: EditorEventCursors,
    },
}

#[derive(Default)]
struct EventQueue {
    events: Vec<EditorEvent>,
    texts: String,
    cursors: Vec<Cursor>,
}

#[derive(Default)]
pub struct EditorEventQueue {
    read: EventQueue,
    write: EventQueue,
}
impl EditorEventQueue {
    pub(crate) fn flip(&mut self) {
        self.read.events.clear();
        self.read.texts.clear();
        std::mem::swap(&mut self.read, &mut self.write);
    }

    pub(crate) fn enqueue(&mut self, event: EditorEvent) {
        self.write.events.push(event);
    }

    pub(crate) fn enqueue_buffer_insert(
        &mut self,
        handle: BufferHandle,
        range: BufferRange,
        text: &str,
    ) {
        let from = self.write.texts.len();
        self.write.texts.push_str(text);
        let text = EditorEventText {
            from: from as _,
            to: self.write.texts.len() as _,
        };
        self.write.events.push(EditorEvent::BufferInsertText {
            handle,
            range,
            text,
        });
    }

    pub fn enqueue_fix_cursors(&mut self, handle: BufferViewHandle, cursors: &[Cursor]) {
        let from = self.write.cursors.len();
        self.write.cursors.extend_from_slice(cursors);
        let cursors = EditorEventCursors {
            from: from as _,
            to: self.write.cursors.len() as _,
        };
        self.write
            .events
            .push(EditorEvent::FixCursors { handle, cursors });
    }
}

pub struct EditorEventIter(usize);
impl EditorEventIter {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn next<'a>(&mut self, queue: &'a EditorEventQueue) -> Option<&'a EditorEvent> {
        let event = queue.read.events.get(self.0)?;
        self.0 += 1;
        Some(event)
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
        write!(f, "{} at char: {}", self.error, self.index)
    }
}
impl Error for KeyParseAllError {}

pub struct KeyParser<'a> {
    chars: Chars<'a>,
    raw: &'a str,
}
impl<'a> KeyParser<'a> {
    pub fn new(raw: &'a str) -> Self {
        Self {
            chars: raw.chars(),
            raw,
        }
    }
}
impl<'a> Iterator for KeyParser<'a> {
    type Item = Result<AnsiKey, KeyParseAllError>;
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
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.raw = "";
                Some(Err(KeyParseAllError { index, error }))
            }
        }
    }
}

fn parse_key(chars: &mut Chars) -> Result<AnsiKey, KeyParseError> {
    fn next(chars: &mut impl Iterator<Item = char>) -> Result<char, KeyParseError> {
        match chars.next() {
            Some(c) => Ok(c),
            None => Err(KeyParseError::UnexpectedEnd),
        }
    }

    fn consume(chars: &mut impl Iterator<Item = char>, c: char) -> Result<(), KeyParseError> {
        let next = next(chars)?;
        if c == next {
            Ok(())
        } else {
            Err(KeyParseError::InvalidCharacter(next))
        }
    }

    fn consume_str(chars: &mut impl Iterator<Item = char>, s: &str) -> Result<(), KeyParseError> {
        for c in s.chars() {
            consume(chars, c)?
        }
        Ok(())
    }

    match next(chars)? {
        '<' => match next(chars)? {
            'b' => {
                consume_str(chars, "ackspace>")?;
                Ok(AnsiKey::Backspace)
            }
            's' => {
                consume_str(chars, "pace>")?;
                Ok(AnsiKey::Char(' '))
            }
            'e' => match next(chars)? {
                'n' => match next(chars)? {
                    't' => {
                        consume_str(chars, "er>")?;
                        Ok(AnsiKey::Enter)
                    }
                    'd' => {
                        consume(chars, '>')?;
                        Ok(AnsiKey::End)
                    }
                    c => Err(KeyParseError::InvalidCharacter(c)),
                },
                's' => {
                    consume_str(chars, "c>")?;
                    Ok(AnsiKey::Esc)
                }
                c => Err(KeyParseError::InvalidCharacter(c)),
            },
            'l' => {
                consume(chars, 'e')?;
                match next(chars)? {
                    's' => {
                        consume_str(chars, "s>")?;
                        Ok(AnsiKey::Char('<'))
                    }
                    'f' => {
                        consume_str(chars, "t>")?;
                        Ok(AnsiKey::Left)
                    }
                    c => Err(KeyParseError::InvalidCharacter(c)),
                }
            }
            'g' => {
                consume_str(chars, "reater>")?;
                Ok(AnsiKey::Char('>'))
            }
            'r' => {
                consume_str(chars, "ight>")?;
                Ok(AnsiKey::Right)
            }
            'u' => {
                consume_str(chars, "p>")?;
                Ok(AnsiKey::Up)
            }
            'd' => match next(chars)? {
                'o' => {
                    consume_str(chars, "wn>")?;
                    Ok(AnsiKey::Down)
                }
                'e' => {
                    consume_str(chars, "lete>")?;
                    Ok(AnsiKey::Delete)
                }
                c => Err(KeyParseError::InvalidCharacter(c)),
            },
            'h' => {
                consume_str(chars, "ome>")?;
                Ok(AnsiKey::Home)
            }
            'p' => {
                consume_str(chars, "age")?;
                match next(chars)? {
                    'u' => {
                        consume_str(chars, "p>")?;
                        Ok(AnsiKey::PageUp)
                    }
                    'd' => {
                        consume_str(chars, "own>")?;
                        Ok(AnsiKey::PageDown)
                    }
                    c => Err(KeyParseError::InvalidCharacter(c)),
                }
            }
            't' => {
                consume_str(chars, "ab>")?;
                Ok(AnsiKey::Tab)
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
                                Ok(AnsiKey::F(n as _))
                            }
                            None => match c {
                                '>' => Ok(AnsiKey::F(d0 as _)),
                                _ => Err(KeyParseError::InvalidCharacter(c)),
                            },
                        }
                    }
                    None => Err(KeyParseError::InvalidCharacter(c)),
                }
            }
            'c' => {
                consume(chars, '-')?;
                let c = next(chars)?;
                if c.is_ascii_alphanumeric() {
                    consume(chars, '>')?;
                    Ok(AnsiKey::Ctrl(c))
                } else {
                    Err(KeyParseError::InvalidCharacter(c))
                }
            }
            'a' => {
                consume(chars, '-')?;
                let c = next(chars)?;
                if c.is_ascii_alphanumeric() {
                    consume(chars, '>')?;
                    Ok(AnsiKey::Alt(c))
                } else {
                    Err(KeyParseError::InvalidCharacter(c))
                }
            }
            c => Err(KeyParseError::InvalidCharacter(c)),
        },
        '>' => Err(KeyParseError::InvalidCharacter('>')),
        c => Ok(AnsiKey::Char(c)),
    }
}

impl fmt::Display for AnsiKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AnsiKey::None => Ok(()),
            AnsiKey::Backspace => f.write_str("<backspace>"),
            AnsiKey::Enter => f.write_str("<enter>"),
            AnsiKey::Left => f.write_str("<left>"),
            AnsiKey::Right => f.write_str("<right>"),
            AnsiKey::Up => f.write_str("<up>"),
            AnsiKey::Down => f.write_str("<down>"),
            AnsiKey::Home => f.write_str("<home>"),
            AnsiKey::End => f.write_str("<end>"),
            AnsiKey::PageUp => f.write_str("<pageup>"),
            AnsiKey::PageDown => f.write_str("<pagedown>"),
            AnsiKey::Tab => f.write_str("<tab>"),
            AnsiKey::Delete => f.write_str("<delete>"),
            AnsiKey::F(n) => write!(f, "<f{}>", n),
            AnsiKey::Char(' ') => f.write_str("<space>"),
            AnsiKey::Char('<') => f.write_str("<less>"),
            AnsiKey::Char('>') => f.write_str("<greater>"),
            AnsiKey::Char(c) => write!(f, "{}", c),
            AnsiKey::Ctrl(c) => write!(f, "<c-{}>", c),
            AnsiKey::Alt(c) => write!(f, "<a-{}>", c),
            AnsiKey::Esc => f.write_str("<esc>"),
        }
    }
}

fn serialize_key<S>(key: AnsiKey, serializer: &mut S)
where
    S: Serializer,
{
    match key {
        AnsiKey::None => 0u8.serialize(serializer),
        AnsiKey::Backspace => 1u8.serialize(serializer),
        AnsiKey::Enter => 2u8.serialize(serializer),
        AnsiKey::Left => 3u8.serialize(serializer),
        AnsiKey::Right => 4u8.serialize(serializer),
        AnsiKey::Up => 5u8.serialize(serializer),
        AnsiKey::Down => 6u8.serialize(serializer),
        AnsiKey::Home => 7u8.serialize(serializer),
        AnsiKey::End => 8u8.serialize(serializer),
        AnsiKey::PageUp => 9u8.serialize(serializer),
        AnsiKey::PageDown => 10u8.serialize(serializer),
        AnsiKey::Tab => 11u8.serialize(serializer),
        AnsiKey::Delete => 12u8.serialize(serializer),
        AnsiKey::F(n) => {
            13u8.serialize(serializer);
            n.serialize(serializer);
        }
        AnsiKey::Char(c) => {
            14u8.serialize(serializer);
            c.serialize(serializer);
        }
        AnsiKey::Ctrl(c) => {
            15u8.serialize(serializer);
            c.serialize(serializer);
        }
        AnsiKey::Alt(c) => {
            16u8.serialize(serializer);
            c.serialize(serializer);
        }
        AnsiKey::Esc => 17u8.serialize(serializer),
    }
}

fn deserialize_key<'de, D>(deserializer: &mut D) -> Result<AnsiKey, DeserializeError>
where
    D: Deserializer<'de>,
{
    let discriminant = u8::deserialize(deserializer)?;
    match discriminant {
        0 => Ok(AnsiKey::None),
        1 => Ok(AnsiKey::Backspace),
        2 => Ok(AnsiKey::Enter),
        3 => Ok(AnsiKey::Left),
        4 => Ok(AnsiKey::Right),
        5 => Ok(AnsiKey::Up),
        6 => Ok(AnsiKey::Down),
        7 => Ok(AnsiKey::Home),
        8 => Ok(AnsiKey::End),
        9 => Ok(AnsiKey::PageUp),
        10 => Ok(AnsiKey::PageDown),
        11 => Ok(AnsiKey::Tab),
        12 => Ok(AnsiKey::Delete),
        13 => {
            let n = Serialize::deserialize(deserializer)?;
            Ok(AnsiKey::F(n))
        }
        14 => {
            let c = Serialize::deserialize(deserializer)?;
            Ok(AnsiKey::Char(c))
        }
        15 => {
            let c = Serialize::deserialize(deserializer)?;
            Ok(AnsiKey::Ctrl(c))
        }
        16 => {
            let c = Serialize::deserialize(deserializer)?;
            Ok(AnsiKey::Alt(c))
        }
        17 => Ok(AnsiKey::Esc),
        _ => Err(DeserializeError::InvalidData),
    }
}

pub enum ServerEvent<'a> {
    Display(&'a [u8]),
    Suspend,
    StdoutOutput(&'a [u8]),
}
impl<'a> ServerEvent<'a> {
    pub const fn bytes_variant_header_len() -> usize {
        1 + std::mem::size_of::<u32>()
    }

    pub fn serialize_bytes_variant_header(&self, buf: &mut [u8]) {
        buf[0] = match self {
            Self::Display(_) => 0,
            Self::Suspend => unreachable!(),
            Self::StdoutOutput(_) => 2,
        };
        let len = buf.len() as u32 - Self::bytes_variant_header_len() as u32;
        let len_buf = len.to_le_bytes();
        buf[1..Self::bytes_variant_header_len()].copy_from_slice(&len_buf);
    }
}
impl<'de> Serialize<'de> for ServerEvent<'de> {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            Self::Display(display) => {
                0u8.serialize(serializer);
                display.serialize(serializer);
            }
            Self::Suspend => 1u8.serialize(serializer),
            Self::StdoutOutput(bytes) => {
                2u8.serialize(serializer);
                bytes.serialize(serializer);
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
                let display = Serialize::deserialize(deserializer)?;
                Ok(Self::Display(display))
            }
            1 => Ok(Self::Suspend),
            2 => {
                let bytes = Serialize::deserialize(deserializer)?;
                Ok(Self::StdoutOutput(bytes))
            }
            _ => Err(DeserializeError::InvalidData),
        }
    }
}

#[derive(Clone, Copy)]
pub enum TargetClient {
    Sender,
    Focused,
}
impl<'de> Serialize<'de> for TargetClient {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            Self::Sender => 0u8.serialize(serializer),
            Self::Focused => 1u8.serialize(serializer),
        }
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        let discriminant = u8::deserialize(deserializer)?;
        match discriminant {
            0 => Ok(Self::Sender),
            1 => Ok(Self::Focused),
            _ => Err(DeserializeError::InvalidData),
        }
    }
}

pub enum ClientEvent<'a> {
    Key(TargetClient, AnsiKey),
    Resize(u16, u16),
    Command(TargetClient, &'a str),
    StdinInput(TargetClient, &'a [u8]),
}
impl<'de> Serialize<'de> for ClientEvent<'de> {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            Self::Key(target, key) => {
                0u8.serialize(serializer);
                target.serialize(serializer);
                serialize_key(*key, serializer);
            }
            Self::Resize(width, height) => {
                1u8.serialize(serializer);
                width.serialize(serializer);
                height.serialize(serializer);
            }
            Self::Command(target, command) => {
                2u8.serialize(serializer);
                target.serialize(serializer);
                command.serialize(serializer);
            }
            Self::StdinInput(target, bytes) => {
                3u8.serialize(serializer);
                target.serialize(serializer);
                bytes.serialize(serializer);
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
                let target = Serialize::deserialize(deserializer)?;
                let key = deserialize_key(deserializer)?;
                Ok(Self::Key(target, key))
            }
            1 => {
                let width = Serialize::deserialize(deserializer)?;
                let height = Serialize::deserialize(deserializer)?;
                Ok(Self::Resize(width, height))
            }
            2 => {
                let target = Serialize::deserialize(deserializer)?;
                let command = Serialize::deserialize(deserializer)?;
                Ok(Self::Command(target, command))
            }
            3 => {
                let target = Serialize::deserialize(deserializer)?;
                let bytes = Serialize::deserialize(deserializer)?;
                Ok(Self::StdinInput(target, bytes))
            }
            _ => Err(DeserializeError::InvalidData),
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
        let mut slice = &buf[self.read_len..];
        if slice.is_empty() {
            return None;
        }

        match ClientEvent::deserialize(&mut slice) {
            Ok(event) => {
                self.read_len = buf.len() - slice.len();
                Some(event)
            }
            Err(_) => None,
        }
    }

    pub fn finish(self, receiver: &mut ClientEventReceiver) {
        receiver.bufs[self.buf_index].drain(..self.read_len);
        std::mem::forget(self);
    }
}
impl Drop for ClientEventIter {
    fn drop(&mut self) {
        panic!("forgot to call 'finish' on ClientEventIter");
    }
}

#[derive(Default)]
pub struct ClientEventReceiver {
    bufs: Vec<Vec<u8>>,
}

impl ClientEventReceiver {
    pub fn len(&self, client_handle: ClientHandle) -> usize {
        self.bufs[client_handle.0 as usize].len()
    }

    pub fn receive_events(&mut self, client_handle: ClientHandle, bytes: &[u8]) -> ClientEventIter {
        let buf_index = client_handle.0 as usize;
        if buf_index >= self.bufs.len() {
            self.bufs.resize_with(buf_index + 1, Vec::new);
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
            AnsiKey::Backspace,
            parse_key(&mut "<backspace>".chars()).unwrap()
        );
        assert_eq!(AnsiKey::Char(' '), parse_key(&mut "<space>".chars()).unwrap());
        assert_eq!(AnsiKey::Enter, parse_key(&mut "<enter>".chars()).unwrap());
        assert_eq!(AnsiKey::Left, parse_key(&mut "<left>".chars()).unwrap());
        assert_eq!(AnsiKey::Right, parse_key(&mut "<right>".chars()).unwrap());
        assert_eq!(AnsiKey::Up, parse_key(&mut "<up>".chars()).unwrap());
        assert_eq!(AnsiKey::Down, parse_key(&mut "<down>".chars()).unwrap());
        assert_eq!(AnsiKey::Home, parse_key(&mut "<home>".chars()).unwrap());
        assert_eq!(AnsiKey::End, parse_key(&mut "<end>".chars()).unwrap());
        assert_eq!(AnsiKey::PageUp, parse_key(&mut "<pageup>".chars()).unwrap());
        assert_eq!(AnsiKey::PageDown, parse_key(&mut "<pagedown>".chars()).unwrap());
        assert_eq!(AnsiKey::Tab, parse_key(&mut "<tab>".chars()).unwrap());
        assert_eq!(AnsiKey::Delete, parse_key(&mut "<delete>".chars()).unwrap());
        assert_eq!(AnsiKey::Esc, parse_key(&mut "<esc>".chars()).unwrap());

        for n in 1..=99 {
            let s = format!("<f{}>", n);
            assert_eq!(AnsiKey::F(n as _), parse_key(&mut s.chars()).unwrap());
        }

        assert_eq!(AnsiKey::Ctrl('z'), parse_key(&mut "<c-z>".chars()).unwrap());
        assert_eq!(AnsiKey::Ctrl('0'), parse_key(&mut "<c-0>".chars()).unwrap());
        assert_eq!(AnsiKey::Ctrl('9'), parse_key(&mut "<c-9>".chars()).unwrap());

        assert_eq!(AnsiKey::Alt('a'), parse_key(&mut "<a-a>".chars()).unwrap());
        assert_eq!(AnsiKey::Alt('z'), parse_key(&mut "<a-z>".chars()).unwrap());
        assert_eq!(AnsiKey::Alt('0'), parse_key(&mut "<a-0>".chars()).unwrap());
        assert_eq!(AnsiKey::Alt('9'), parse_key(&mut "<a-9>".chars()).unwrap());

        assert_eq!(AnsiKey::Char('a'), parse_key(&mut "a".chars()).unwrap());
        assert_eq!(AnsiKey::Char('z'), parse_key(&mut "z".chars()).unwrap());
        assert_eq!(AnsiKey::Char('0'), parse_key(&mut "0".chars()).unwrap());
        assert_eq!(AnsiKey::Char('9'), parse_key(&mut "9".chars()).unwrap());
        assert_eq!(AnsiKey::Char('_'), parse_key(&mut "_".chars()).unwrap());
        assert_eq!(AnsiKey::Char('<'), parse_key(&mut "<less>".chars()).unwrap());
        assert_eq!(AnsiKey::Char('>'), parse_key(&mut "<greater>".chars()).unwrap());
        assert_eq!(AnsiKey::Char('\\'), parse_key(&mut "\\".chars()).unwrap());
    }

    #[test]
    fn key_serialization() {
        fn assert_key_serialization(key: AnsiKey) {
            let mut buf = Vec::new();
            let _ = serialize_key(key, &mut buf);
            let mut slice = buf.as_slice();
            assert!(!slice.is_empty());
            match deserialize_key(&mut slice) {
                Ok(k) => assert_eq!(key, k),
                Err(_) => assert!(false),
            }
        }

        assert_key_serialization(AnsiKey::None);
        assert_key_serialization(AnsiKey::Backspace);
        assert_key_serialization(AnsiKey::Enter);
        assert_key_serialization(AnsiKey::Left);
        assert_key_serialization(AnsiKey::Right);
        assert_key_serialization(AnsiKey::Up);
        assert_key_serialization(AnsiKey::Down);
        assert_key_serialization(AnsiKey::Home);
        assert_key_serialization(AnsiKey::End);
        assert_key_serialization(AnsiKey::PageUp);
        assert_key_serialization(AnsiKey::PageDown);
        assert_key_serialization(AnsiKey::Tab);
        assert_key_serialization(AnsiKey::Delete);
        assert_key_serialization(AnsiKey::F(0));
        assert_key_serialization(AnsiKey::F(9));
        assert_key_serialization(AnsiKey::F(12));
        assert_key_serialization(AnsiKey::F(99));
        assert_key_serialization(AnsiKey::Char('a'));
        assert_key_serialization(AnsiKey::Char('z'));
        assert_key_serialization(AnsiKey::Char('A'));
        assert_key_serialization(AnsiKey::Char('Z'));
        assert_key_serialization(AnsiKey::Char('0'));
        assert_key_serialization(AnsiKey::Char('9'));
        assert_key_serialization(AnsiKey::Char('$'));
        assert_key_serialization(AnsiKey::Ctrl('a'));
        assert_key_serialization(AnsiKey::Ctrl('z'));
        assert_key_serialization(AnsiKey::Ctrl('A'));
        assert_key_serialization(AnsiKey::Ctrl('Z'));
        assert_key_serialization(AnsiKey::Ctrl('0'));
        assert_key_serialization(AnsiKey::Ctrl('9'));
        assert_key_serialization(AnsiKey::Ctrl('$'));
        assert_key_serialization(AnsiKey::Alt('a'));
        assert_key_serialization(AnsiKey::Alt('z'));
        assert_key_serialization(AnsiKey::Alt('A'));
        assert_key_serialization(AnsiKey::Alt('Z'));
        assert_key_serialization(AnsiKey::Alt('0'));
        assert_key_serialization(AnsiKey::Alt('9'));
        assert_key_serialization(AnsiKey::Alt('$'));
        assert_key_serialization(AnsiKey::Esc);
    }

    #[test]
    fn client_event_deserialize_splitted() {
        const CHAR: char = 'x';
        const EVENT_COUNT: usize = 100;

        fn check_next_event(events: &mut ClientEventIter, receiver: &ClientEventReceiver) -> bool {
            match events.next(receiver) {
                Some(ClientEvent::Key(_, AnsiKey::Char(CHAR))) => true,
                Some(ClientEvent::Key(_, AnsiKey::Char(c))) => {
                    panic!("received char {} instead of {}", c, CHAR);
                }
                Some(event) => panic!(
                    "received other kind of event. discriminant: {:?}",
                    std::mem::discriminant(&event),
                ),
                None => false,
            }
        }

        let client_handle = ClientHandle(0);
        let event = ClientEvent::Key(TargetClient::Sender, AnsiKey::Char(CHAR));
        let mut bytes = Vec::new();
        for _ in 0..EVENT_COUNT {
            event.serialize(&mut bytes);
        }
        assert_eq!(700, bytes.len());

        let mut event_count = 0;
        let mut receiver = ClientEventReceiver::default();

        let mut events = receiver.receive_events(client_handle, &bytes[..512]);
        while check_next_event(&mut events, &receiver) {
            event_count += 1;
        }
        assert_eq!(511, events.read_len);
        events.finish(&mut receiver);

        let mut events = receiver.receive_events(client_handle, &bytes[512..]);
        while check_next_event(&mut events, &receiver) {
            event_count += 1;
        }
        events.finish(&mut receiver);

        assert_eq!(0, receiver.bufs[client_handle.0 as usize].len());
        assert_eq!(EVENT_COUNT, event_count);
    }
}
