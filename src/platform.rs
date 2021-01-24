use std::{
    error::Error,
    fmt, io,
    process::{Command, ExitStatus},
    str::Chars,
};

use crate::{
    client::TargetClient,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
};

#[cfg(windows)]
mod windows;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
                    consume!('e');
                    match next!() {
                        's' => {
                            consume_str!("s>");
                            Key::Char('<')
                        }
                        'f' => {
                            consume_str!("t>");
                            Key::Left
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    }
                }
                'g' => {
                    consume_str!("reater>");
                    Key::Char('>')
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
                    let c = next!();
                    match c.to_digit(10) {
                        Some(d0) => {
                            let c = next!();
                            match c.to_digit(10) {
                                Some(d1) => {
                                    consume!('>');
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
            c @ '>' => return Err(KeyParseError::InvalidCharacter(c)),
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ConnectionHandle(pub(crate) usize);
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcessHandle(pub(crate) usize);

pub enum ServerEvent<'a> {
    Idle,
    ConnectionOpen(ConnectionHandle),
    ConnectionClose(ConnectionHandle),
    ConnectionMessage(ConnectionHandle, &'a [u8]),
    ProcessStdout(ProcessHandle, &'a [u8]),
    ProcessStderr(ProcessHandle, &'a [u8]),
    ProcessExit(ProcessHandle, ExitStatus),
}

pub enum ClientEvent<'a> {
    Resize(usize, usize),
    Key(Key),
    Message(&'a [u8]),
}

pub trait ServerApplication: Sized {
    fn new() -> Option<Self>;
    fn on_event<P>(&mut self, platform: &mut P, event: ServerEvent) -> bool
    where
        P: Platform;
}

pub trait ClientApplication: Sized {
    fn new() -> Option<Self>;
    fn on_event(&mut self, event: ClientEvent) -> &[u8];
}

pub trait Platform {
    fn write_to_connection(&mut self, handle: ConnectionHandle, buf: &[u8]) -> bool;
    fn spawn_process(&mut self, command: Command) -> io::Result<ProcessHandle>;
    fn write_to_process(&mut self, handle: ProcessHandle, buf: &[u8]) -> bool;
}

pub fn run<S, C>()
where
    S: ServerApplication,
    C: ClientApplication,
{
    #[cfg(windows)]
    {
        windows::run::<S, C>();
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

        for n in 1..=99 {
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
        assert_eq!(Key::Char('<'), Key::parse(&mut "<less>".chars()).unwrap());
        assert_eq!(
            Key::Char('>'),
            Key::parse(&mut "<greater>".chars()).unwrap()
        );
        assert_eq!(Key::Char('\\'), Key::parse(&mut "\\".chars()).unwrap());
    }

    #[test]
    fn key_serialization() {
        use crate::serialization::{DeserializationSlice, SerializationBuf};

        macro_rules! assert_key_serialization {
            ($key:expr) => {
                let mut buf = SerializationBuf::default();
                let _ = $key.serialize(&mut buf);
                let slice = buf.as_slice();
                let mut deserializer = DeserializationSlice::from_slice(slice);
                assert!(!deserializer.as_slice().is_empty());
                match Key::deserialize(&mut deserializer) {
                    Ok(key) => assert_eq!($key, key),
                    Err(_) => assert!(false),
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
        assert_key_serialization!(Key::F(99));
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
