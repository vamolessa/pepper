use std::{error::Error, fmt, str::Chars};

use crate::{
    buffer::BufferHandle,
    buffer_position::BufferRange,
    buffer_view::BufferViewHandle,
    client::ClientHandle,
    cursor::Cursor,
    platform::{Key, KeyCode},
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

    pub fn fix_cursors_mut_guard(&mut self, handle: BufferViewHandle) -> FixCursorMutGuard {
        let previous_cursors_len = self.write.cursors.len() as _;
        FixCursorMutGuard {
            inner: &mut self.write,
            handle,
            previous_cursors_len,
        }
    }
}

pub struct FixCursorMutGuard<'a> {
    inner: &'a mut EventQueue,
    handle: BufferViewHandle,
    previous_cursors_len: u32,
}
impl<'a> FixCursorMutGuard<'a> {
    pub fn cursors(&mut self) -> &mut Vec<Cursor> {
        &mut self.inner.cursors
    }
}
impl<'a> Drop for FixCursorMutGuard<'a> {
    fn drop(&mut self) {
        if self.inner.cursors.len() < self.previous_cursors_len as _ {
            panic!("deleted too many cursors on `EditorEventQueue::fix_cursors_mut_guard`");
        }

        let cursors = EditorEventCursors {
            from: self.previous_cursors_len,
            to: self.inner.cursors.len() as _,
        };
        self.inner.events.push(EditorEvent::FixCursors {
            handle: self.handle,
            cursors,
        });
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
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.raw = "";
                Some(Err(KeyParseAllError { index, error }))
            }
        }
    }
}

fn parse_key(chars: &mut Chars) -> Result<Key, KeyParseError> {
    fn next(chars: &mut Chars) -> Result<char, KeyParseError> {
        match chars.next() {
            Some(c) => Ok(c),
            None => Err(KeyParseError::UnexpectedEnd),
        }
    }

    fn consume(chars: &mut Chars, c: char) -> Result<(), KeyParseError> {
        let next = next(chars)?;
        if c == next {
            Ok(())
        } else {
            Err(KeyParseError::InvalidCharacter(next))
        }
    }

    fn consume_str(chars: &mut Chars, s: &str) -> Result<(), KeyParseError> {
        for c in s.chars() {
            consume(chars, c)?
        }
        Ok(())
    }

    fn check_modifier(chars: &mut Chars, c: char) -> bool {
        let saved = chars.clone();
        if chars.next() == Some(c) {
            if chars.next() == Some('-') {
                return true;
            }
        }

        *chars = saved;
        false
    }

    match next(chars)? {
        '<' => {
            let mut shift = check_modifier(chars, 's');
            let control = check_modifier(chars, 'c');
            let alt = check_modifier(chars, 'a');

            let code = match next(chars)? {
                'b' => match next(chars)? {
                    'a' => {
                        consume_str(chars, "ckspace>")?;
                        KeyCode::Backspace
                    }
                    '>' => KeyCode::Char('b'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                's' => match next(chars)? {
                    'p' => {
                        consume_str(chars, "ace>")?;
                        KeyCode::Char(' ')
                    }
                    '>' => KeyCode::Char('s'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'e' => match next(chars)? {
                    'n' => match next(chars)? {
                        't' => {
                            consume_str(chars, "er>")?;
                            KeyCode::Char('\n')
                        }
                        'd' => {
                            consume(chars, '>')?;
                            KeyCode::End
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    },
                    's' => {
                        consume_str(chars, "c>")?;
                        KeyCode::Esc
                    }
                    '>' => KeyCode::Char('e'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'l' => match next(chars)? {
                    'e' => match next(chars)? {
                        's' => {
                            consume_str(chars, "s>")?;
                            KeyCode::Char('<')
                        }
                        'f' => {
                            consume_str(chars, "t>")?;
                            KeyCode::Left
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    },
                    '>' => KeyCode::Char('l'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'g' => match next(chars)? {
                    'r' => {
                        consume_str(chars, "eater>")?;
                        KeyCode::Char('>')
                    }
                    '>' => KeyCode::Char('g'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'r' => match next(chars)? {
                    'i' => {
                        consume_str(chars, "ght>")?;
                        KeyCode::Right
                    }
                    '>' => KeyCode::Char('r'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'u' => match next(chars)? {
                    'p' => {
                        consume(chars, '>')?;
                        KeyCode::Up
                    }
                    '>' => KeyCode::Char('l'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'd' => match next(chars)? {
                    'o' => {
                        consume_str(chars, "wn>")?;
                        KeyCode::Down
                    }
                    'e' => {
                        consume_str(chars, "lete>")?;
                        KeyCode::Delete
                    }
                    '>' => KeyCode::Char('d'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'h' => match next(chars)? {
                    'o' => {
                        consume_str(chars, "me>")?;
                        KeyCode::Home
                    }
                    '>' => KeyCode::Char('h'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'p' => match next(chars)? {
                    'a' => {
                        consume_str(chars, "ge")?;
                        match next(chars)? {
                            'u' => {
                                consume_str(chars, "p>")?;
                                KeyCode::PageUp
                            }
                            'd' => {
                                consume_str(chars, "own>")?;
                                KeyCode::PageDown
                            }
                            c => return Err(KeyParseError::InvalidCharacter(c)),
                        }
                    }
                    '>' => KeyCode::Char('p'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                't' => match next(chars)? {
                    'a' => {
                        consume_str(chars, "b>")?;
                        KeyCode::Char('\t')
                    }
                    '>' => KeyCode::Char('t'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'f' => match next(chars)? {
                    c @ '0'..='9' => match c.to_digit(10) {
                        Some(d0) => {
                            let c = next(chars)?;
                            match c.to_digit(10) {
                                Some(d1) => {
                                    consume(chars, '>')?;
                                    let n = d0 * 10 + d1;
                                    KeyCode::F(n as _)
                                }
                                None => match c {
                                    '>' => KeyCode::F(d0 as _),
                                    _ => return Err(KeyParseError::InvalidCharacter(c)),
                                },
                            }
                        }
                        None => return Err(KeyParseError::InvalidCharacter(c)),
                    },
                    '>' => KeyCode::Char('f'),
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                mut c => {
                    consume(chars, '>')?;
                    if shift {
                        c = c.to_ascii_uppercase();
                    } else {
                        shift = c.is_ascii_uppercase();
                    }
                    KeyCode::Char(c)
                }
            };

            Ok(Key {
                code,
                shift,
                control,
                alt,
            })
        }
        '>' => Err(KeyParseError::InvalidCharacter('>')),
        c => Ok(Key {
            code: KeyCode::Char(c),
            shift: c.is_ascii_uppercase(),
            control: false,
            alt: false,
        }),
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.control
            && !self.alt
            && !matches!(self.code, KeyCode::Char('\n' | '\t' | ' ' | '<' | '>'))
        {
            if let KeyCode::Char(c) = self.code {
                return write!(f, "{}", c);
            }
        }

        f.write_str("<")?;
        if self.shift {
            f.write_str("s-")?;
        }
        if self.control {
            f.write_str("c-")?;
        }
        if self.alt {
            f.write_str("a-")?;
        }
        match self.code {
            KeyCode::None => f.write_str("none")?,
            KeyCode::Backspace => f.write_str("backspace")?,
            KeyCode::Left => f.write_str("left")?,
            KeyCode::Right => f.write_str("right")?,
            KeyCode::Up => f.write_str("up")?,
            KeyCode::Down => f.write_str("down")?,
            KeyCode::Home => f.write_str("home")?,
            KeyCode::End => f.write_str("end")?,
            KeyCode::PageUp => f.write_str("pageup")?,
            KeyCode::PageDown => f.write_str("pagedown")?,
            KeyCode::Delete => f.write_str("delete")?,
            KeyCode::F(n) => write!(f, "f{}", n)?,
            KeyCode::Char('\n') => f.write_str("enter")?,
            KeyCode::Char('\t') => f.write_str("tab")?,
            KeyCode::Char(' ') => f.write_str("space")?,
            KeyCode::Char('<') => f.write_str("less")?,
            KeyCode::Char('>') => f.write_str("greater")?,
            KeyCode::Char(c) => write!(f, "{}", c)?,
            KeyCode::Esc => f.write_str("esc")?,
        }
        f.write_str(">")?;
        Ok(())
    }
}

fn serialize_key<S>(key: Key, serializer: &mut S)
where
    S: Serializer,
{
    let mut flags = 0u8;
    flags |= (key.shift as u8) << 0;
    flags |= (key.control as u8) << 1;
    flags |= (key.alt as u8) << 2;
    flags.serialize(serializer);

    match key.code {
        KeyCode::None => 0u8.serialize(serializer),
        KeyCode::Backspace => 1u8.serialize(serializer),
        KeyCode::Left => 2u8.serialize(serializer),
        KeyCode::Right => 3u8.serialize(serializer),
        KeyCode::Up => 4u8.serialize(serializer),
        KeyCode::Down => 5u8.serialize(serializer),
        KeyCode::Home => 6u8.serialize(serializer),
        KeyCode::End => 7u8.serialize(serializer),
        KeyCode::PageUp => 8u8.serialize(serializer),
        KeyCode::PageDown => 9u8.serialize(serializer),
        KeyCode::Delete => 10u8.serialize(serializer),
        KeyCode::F(n) => {
            11u8.serialize(serializer);
            n.serialize(serializer);
        }
        KeyCode::Char(c) => {
            12u8.serialize(serializer);
            c.serialize(serializer);
        }
        KeyCode::Esc => 13u8.serialize(serializer),
    }
}

fn deserialize_key<'de, D>(deserializer: &mut D) -> Result<Key, DeserializeError>
where
    D: Deserializer<'de>,
{
    let flags = u8::deserialize(deserializer)?;
    let shift = (flags & 0b001) != 0;
    let control = (flags & 0b010) != 0;
    let alt = (flags & 0b100) != 0;

    let code_discriminant = u8::deserialize(deserializer)?;
    let code = match code_discriminant {
        0 => KeyCode::None,
        1 => KeyCode::Backspace,
        2 => KeyCode::Left,
        3 => KeyCode::Right,
        4 => KeyCode::Up,
        5 => KeyCode::Down,
        6 => KeyCode::Home,
        7 => KeyCode::End,
        8 => KeyCode::PageUp,
        9 => KeyCode::PageDown,
        10 => KeyCode::Delete,
        11 => {
            let n = Serialize::deserialize(deserializer)?;
            KeyCode::F(n)
        }
        12 => {
            let c = Serialize::deserialize(deserializer)?;
            KeyCode::Char(c)
        }
        13 => KeyCode::Esc,
        _ => return Err(DeserializeError::InvalidData),
    };

    Ok(Key {
        code,
        shift,
        control,
        alt,
    })
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
    Key(TargetClient, Key),
    Resize(u16, u16),
    Commands(TargetClient, &'a str),
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
            Self::Commands(target, command) => {
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
                Ok(Self::Commands(target, command))
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
        fn assert_key_simple(expected_code: KeyCode, text: &str) {
            let parsed = parse_key(&mut text.chars()).unwrap();
            assert_eq!(expected_code, parsed.code);
            assert!(!parsed.shift);
            assert!(!parsed.control);
            assert!(!parsed.alt);
        }

        assert_key_simple(KeyCode::Backspace, "<backspace>");
        assert_key_simple(KeyCode::Char(' '), "<space>");
        assert_key_simple(KeyCode::Char('\n'), "<enter>");
        assert_key_simple(KeyCode::Left, "<left>");
        assert_key_simple(KeyCode::Right, "<right>");
        assert_key_simple(KeyCode::Up, "<up>");
        assert_key_simple(KeyCode::Down, "<down>");
        assert_key_simple(KeyCode::Home, "<home>");
        assert_key_simple(KeyCode::End, "<end>");
        assert_key_simple(KeyCode::PageUp, "<pageup>");
        assert_key_simple(KeyCode::PageDown, "<pagedown>");
        assert_key_simple(KeyCode::Char('\t'), "<tab>");
        assert_key_simple(KeyCode::Delete, "<delete>");
        assert_key_simple(KeyCode::Esc, "<esc>");

        for n in 1..=99 {
            let s = format!("<f{}>", n);
            assert_key_simple(KeyCode::F(n as _), &s);
        }

        assert_key_simple(KeyCode::Char('a'), "a");
        assert_key_simple(KeyCode::Char('z'), "z");
        assert_key_simple(KeyCode::Char('0'), "0");
        assert_key_simple(KeyCode::Char('9'), "9");
        assert_key_simple(KeyCode::Char('_'), "_");
        assert_key_simple(KeyCode::Char('<'), "<less>");
        assert_key_simple(KeyCode::Char('>'), "<greater>");
        assert_key_simple(KeyCode::Char('\\'), "\\");
        assert_key_simple(KeyCode::Char('!'), "!");
        assert_key_simple(KeyCode::Char('|'), "|");

        fn assert_key_with_modifiers(expected_code: KeyCode, control: bool, alt: bool, text: &str) {
            let parsed = parse_key(&mut text.chars()).unwrap();
            assert_eq!(expected_code, parsed.code);
            assert!(!parsed.shift);
            assert_eq!(control, parsed.control);
            assert_eq!(alt, parsed.alt);
        }

        assert_key_with_modifiers(KeyCode::Char('c'), true, false, "<c-c>");
        assert_key_with_modifiers(KeyCode::Char('z'), true, false, "<c-z>");
        assert_key_with_modifiers(KeyCode::Char('s'), true, false, "<c-s>");
        assert_key_with_modifiers(KeyCode::Char('0'), true, false, "<c-0>");
        assert_key_with_modifiers(KeyCode::Char('9'), true, false, "<c-9>");

        assert_key_with_modifiers(KeyCode::Char('a'), false, true, "<a-a>");
        assert_key_with_modifiers(KeyCode::Char('z'), false, true, "<a-z>");
        assert_key_with_modifiers(KeyCode::Char('s'), false, true, "<a-s>");
        assert_key_with_modifiers(KeyCode::Char('0'), false, true, "<a-0>");
        assert_key_with_modifiers(KeyCode::Char('9'), false, true, "<a-9>");

        assert_key_with_modifiers(KeyCode::Char('a'), true, true, "<c-a-a>");
        assert_key_with_modifiers(KeyCode::Char('c'), true, true, "<c-a-c>");
        assert_key_with_modifiers(KeyCode::Char('s'), true, true, "<c-a-s>");
    }

    #[test]
    fn key_serialization() {
        fn assert_key_serialization(buf: &mut Vec<u8>, code: KeyCode) {
            fn check(buf: &mut Vec<u8>, code: KeyCode, shift: bool, control: bool, alt: bool) {
                let shift = match code {
                    KeyCode::Char(c) if c.is_ascii_uppercase() => true,
                    _ => shift,
                };
                let key = Key {
                    code,
                    shift,
                    control,
                    alt,
                };
                buf.clear();
                let _ = serialize_key(key, buf);
                let mut slice = buf.as_slice();
                assert!(!slice.is_empty());
                match deserialize_key(&mut slice) {
                    Ok(k) => assert_eq!(key, k),
                    Err(_) => assert!(false),
                }
                assert!(slice.is_empty());
            }

            for s in 0..=1 {
                for c in 0..=1 {
                    for a in 0..=1 {
                        check(buf, code, s == 1, c == 1, a == 1);
                    }
                }
            }
        }

        let mut buf = Vec::new();

        assert_key_serialization(&mut buf, KeyCode::None);
        assert_key_serialization(&mut buf, KeyCode::Backspace);
        assert_key_serialization(&mut buf, KeyCode::Left);
        assert_key_serialization(&mut buf, KeyCode::Right);
        assert_key_serialization(&mut buf, KeyCode::Up);
        assert_key_serialization(&mut buf, KeyCode::Down);
        assert_key_serialization(&mut buf, KeyCode::Home);
        assert_key_serialization(&mut buf, KeyCode::End);
        assert_key_serialization(&mut buf, KeyCode::PageUp);
        assert_key_serialization(&mut buf, KeyCode::PageDown);
        assert_key_serialization(&mut buf, KeyCode::Delete);
        assert_key_serialization(&mut buf, KeyCode::F(0));
        assert_key_serialization(&mut buf, KeyCode::F(9));
        assert_key_serialization(&mut buf, KeyCode::F(12));
        assert_key_serialization(&mut buf, KeyCode::F(99));
        assert_key_serialization(&mut buf, KeyCode::Char('a'));
        assert_key_serialization(&mut buf, KeyCode::Char('z'));
        assert_key_serialization(&mut buf, KeyCode::Char('A'));
        assert_key_serialization(&mut buf, KeyCode::Char('Z'));
        assert_key_serialization(&mut buf, KeyCode::Char('0'));
        assert_key_serialization(&mut buf, KeyCode::Char('9'));
        assert_key_serialization(&mut buf, KeyCode::Char('$'));
        assert_key_serialization(&mut buf, KeyCode::Char('!'));
        assert_key_serialization(&mut buf, KeyCode::Char('|'));
        assert_key_serialization(&mut buf, KeyCode::Char('\n'));
        assert_key_serialization(&mut buf, KeyCode::Char('\t'));
        assert_key_serialization(&mut buf, KeyCode::Esc);
    }

    #[test]
    fn client_event_deserialize_splitted() {
        const KEY: Key = Key {
            code: KeyCode::Char('x'),
            shift: false,
            control: false,
            alt: false,
        };
        const EVENT_COUNT: usize = 100;

        fn check_next_event(events: &mut ClientEventIter, receiver: &ClientEventReceiver) -> bool {
            match events.next(receiver) {
                Some(ClientEvent::Key(_, KEY)) => true,
                Some(event) => panic!(
                    "received other kind of event. discriminant: {:?}",
                    std::mem::discriminant(&event),
                ),
                None => false,
            }
        }

        let client_handle = ClientHandle(0);
        let event = ClientEvent::Key(TargetClient::Sender, KEY);
        let mut bytes = Vec::new();
        for _ in 0..EVENT_COUNT {
            event.serialize(&mut bytes);
        }
        assert_eq!(800, bytes.len());

        struct Events<'a> {
            pub receiver: &'a mut ClientEventReceiver,
            pub events: Option<ClientEventIter>,
        }
        impl<'a> Events<'a> {
            pub fn check_next_event(&mut self) -> bool {
                check_next_event(self.events.as_mut().unwrap(), &self.receiver)
            }

            pub fn read_len(&self) -> usize {
                self.events.as_ref().unwrap().read_len
            }
        }
        impl<'a> Drop for Events<'a> {
            fn drop(&mut self) {
                if let Some(events) = self.events.take() {
                    events.finish(self.receiver);
                }
            }
        }

        let mut event_count = 0;
        let mut receiver = ClientEventReceiver::default();

        const FIRST_READ_LEN: usize = 496;

        {
            let events = receiver.receive_events(client_handle, &bytes[..FIRST_READ_LEN]);
            let mut events = Events {
                receiver: &mut receiver,
                events: Some(events),
            };

            while events.check_next_event() {
                event_count += 1;
            }
            assert_eq!(FIRST_READ_LEN, events.read_len());
        }

        {
            let events = receiver.receive_events(client_handle, &bytes[FIRST_READ_LEN..]);
            let mut events = Events {
                receiver: &mut receiver,
                events: Some(events),
            };

            while events.check_next_event() {
                event_count += 1;
            }
        }

        assert_eq!(0, receiver.bufs[client_handle.0 as usize].len());
        assert_eq!(EVENT_COUNT, event_count);
    }

    #[test]
    fn key_parser() {
        fn assert_key(expect_code: KeyCode, expect_control: bool, key: Key) {
            assert_eq!(expect_code, key.code);
            assert_eq!(false, key.shift);
            assert_eq!(expect_control, key.control);
            assert_eq!(false, key.alt);
        }

        let mut parser = KeyParser::new("<c-c>");
        assert_key(KeyCode::Char('c'), true, parser.next().unwrap().unwrap());
        assert!(parser.next().is_none());

        let mut parser = KeyParser::new("a<enter><c-c>d<c-space>");
        assert_key(KeyCode::Char('a'), false, parser.next().unwrap().unwrap());
        assert_key(KeyCode::Char('\n'), false, parser.next().unwrap().unwrap());
        assert_key(KeyCode::Char('c'), true, parser.next().unwrap().unwrap());
        assert_key(KeyCode::Char('d'), false, parser.next().unwrap().unwrap());
        assert_key(KeyCode::Char(' '), true, parser.next().unwrap().unwrap());
        assert!(parser.next().is_none());
    }
}

