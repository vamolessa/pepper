use std::{fmt, process::Command};

use crate::{
    buffer::char_display_len,
    command::{CommandManager, CommandTokenizer},
    editor::{BufferedKeys, EditorContext, EditorFlow, KeysIterator},
    events::{KeyParseAllError, KeyParser},
    mode::ModeKind,
    platform::{Key, Platform},
    word_database::{WordIter, WordKind},
};

pub enum MatchResult<'a> {
    None,
    Prefix,
    ReplaceWith(&'a [Key]),
}

#[derive(Debug)]
pub enum ParseKeyMapError {
    CantRemapPluginMode,
    From(KeyParseAllError),
    To(KeyParseAllError),
}
impl fmt::Display for ParseKeyMapError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::CantRemapPluginMode => write!(f, "can not remap plugin mode"),
            Self::From(error) => write!(f, "invalid 'from' binding '{}'", error),
            Self::To(error) => write!(f, "invalid 'to' binding '{}'", error),
        }
    }
}

struct KeyMap {
    from: Vec<Key>,
    to: Vec<Key>,
}

#[derive(Default)]
pub struct KeyMapCollection {
    maps: [Vec<KeyMap>; 5],
}

impl KeyMapCollection {
    pub fn parse_and_map(
        &mut self,
        mode: ModeKind,
        from: &str,
        to: &str,
    ) -> Result<(), ParseKeyMapError> {
        fn parse_keys(text: &str) -> Result<Vec<Key>, KeyParseAllError> {
            let mut keys = Vec::new();
            for key in KeyParser::new(text) {
                match key {
                    Ok(key) => keys.push(key),
                    Err(error) => return Err(error),
                }
            }
            Ok(keys)
        }

        if let ModeKind::Plugin = mode {
            return Err(ParseKeyMapError::CantRemapPluginMode);
        }

        let map = KeyMap {
            from: parse_keys(from).map_err(ParseKeyMapError::From)?,
            to: parse_keys(to).map_err(ParseKeyMapError::To)?,
        };

        let maps = &mut self.maps[mode as usize];
        for m in maps.iter_mut() {
            if m.from == map.from {
                m.to = map.to;
                return Ok(());
            }
        }

        maps.push(map);
        Ok(())
    }

    pub fn matches(&self, mode: ModeKind, keys: &[Key]) -> MatchResult<'_> {
        if let ModeKind::Plugin = mode {
            return MatchResult::None;
        }

        let maps = &self.maps[mode as usize];

        let mut has_prefix = false;
        for map in maps {
            if map.from.iter().zip(keys.iter()).all(|(a, b)| a == b) {
                has_prefix = true;
                if map.from.len() == keys.len() {
                    return MatchResult::ReplaceWith(&map.to);
                }
            }
        }

        if has_prefix {
            MatchResult::Prefix
        } else {
            MatchResult::None
        }
    }
}

#[derive(Clone, Copy)]
pub enum ReadLinePoll {
    Pending,
    Submitted,
    Canceled,
}

#[derive(Default)]
pub struct ReadLine {
    prompt: String,
    input: String,
}
impl ReadLine {
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn set_prompt(&mut self, prompt: &str) {
        self.prompt.clear();
        self.prompt.push_str(prompt);
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut String {
        &mut self.input
    }

    pub fn poll(
        &mut self,
        platform: &mut Platform,
        string_pool: &mut StringPool,
        buffered_keys: &BufferedKeys,
        keys_iter: &mut KeysIterator,
    ) -> ReadLinePoll {
        match keys_iter.next(buffered_keys) {
            Key::Esc | Key::Ctrl('c') => ReadLinePoll::Canceled,
            Key::Enter | Key::Ctrl('m') => ReadLinePoll::Submitted,
            Key::Home | Key::Ctrl('u') => {
                self.input.clear();
                ReadLinePoll::Pending
            }
            Key::Ctrl('w') => {
                let mut words = WordIter(&self.input);
                (&mut words)
                    .filter(|w| w.kind == WordKind::Identifier)
                    .next_back();
                let len = words.0.len();
                self.input.truncate(len);
                ReadLinePoll::Pending
            }
            Key::Backspace | Key::Ctrl('h') => {
                if let Some((last_char_index, _)) = self.input.char_indices().next_back() {
                    self.input.truncate(last_char_index);
                }
                ReadLinePoll::Pending
            }
            Key::Ctrl('y') => {
                let mut text = string_pool.acquire();
                platform.read_from_clipboard(&mut text);
                self.input.push_str(&text);
                string_pool.release(text);
                ReadLinePoll::Pending
            }
            Key::Char(c) => {
                self.input.push(c);
                ReadLinePoll::Pending
            }
            _ => ReadLinePoll::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MessageKind {
    Info,
    Error,
}

pub struct StatusBar {
    kind: MessageKind,
    message: String,
}
impl StatusBar {
    pub fn new() -> Self {
        Self {
            kind: MessageKind::Info,
            message: String::new(),
        }
    }

    pub fn message(&self) -> (MessageKind, &str) {
        (self.kind, &self.message)
    }

    pub fn clear(&mut self) {
        self.message.clear();
    }

    pub fn write(&mut self, kind: MessageKind) -> StatusBarWriter {
        self.kind = kind;
        self.message.clear();
        StatusBarWriter(&mut self.message)
    }

    pub fn extra_height(&self, available_size: (u16, u8)) -> usize {
        let mut height = matches!(self.kind, MessageKind::Error) as _;
        let mut x = 0;

        for c in self.message.chars() {
            match c {
                '\n' => {
                    height += 1;
                    if height > available_size.1 as _ {
                        break;
                    }
                }
                c => {
                    x += char_display_len(c) as usize;
                    if x > available_size.0 as _ {
                        x -= available_size.0 as usize;
                        height += 1;
                        if height > available_size.1 as _ {
                            break;
                        }
                    }
                }
            }
        }

        height
    }
}
pub struct StatusBarWriter<'a>(&'a mut String);
impl<'a> StatusBarWriter<'a> {
    pub fn str(&mut self, message: &str) {
        self.0.push_str(message);
    }

    pub fn fmt(&mut self, args: fmt::Arguments) {
        let _ = fmt::write(&mut self.0, args);
    }
}

#[derive(Default)]
pub struct StringPool {
    pool: Vec<String>,
}
impl StringPool {
    pub fn acquire(&mut self) -> String {
        match self.pool.pop() {
            Some(s) => s,
            None => String::new(),
        }
    }

    pub fn acquire_with(&mut self, value: &str) -> String {
        match self.pool.pop() {
            Some(mut s) => {
                s.push_str(value);
                s
            }
            None => String::from(value),
        }
    }

    pub fn release(&mut self, mut s: String) {
        s.clear();
        self.pool.push(s);
    }
}

pub static SEARCH_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('s');
pub static AUTO_MACRO_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('a');

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegisterKey(u8);

impl RegisterKey {
    const fn from_char_unchecked(key: char) -> Self {
        let key = key as u8;
        Self(key - b'a')
    }

    pub const fn from_char(key: char) -> Option<Self> {
        let key = key as u8;
        if key >= b'a' && key <= b'z' {
            Some(Self(key - b'a'))
        } else {
            None
        }
    }

    pub fn as_u8(&self) -> u8 {
        self.0 + b'a'
    }
}

const REGISTERS_LEN: usize = (b'z' - b'a' + 1) as _;

pub struct RegisterCollection {
    registers: [String; REGISTERS_LEN],
}

impl RegisterCollection {
    pub const fn new() -> Self {
        const DEFAULT_STRING: String = String::new();
        Self {
            registers: [DEFAULT_STRING; REGISTERS_LEN],
        }
    }

    pub fn get(&self, key: RegisterKey) -> &str {
        &self.registers[key.0 as usize]
    }

    pub fn get_mut(&mut self, key: RegisterKey) -> &mut String {
        &mut self.registers[key.0 as usize]
    }
}

// FNV-1a : https://en.wikipedia.org/wiki/Fowler–Noll–Vo_hash_function
pub const fn hash_bytes(mut bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    while let [b, rest @ ..] = bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        bytes = rest;
    }
    hash
}

// extracted from str::is_char_boundary(&self, index: usize) -> bool
// https://doc.rust-lang.org/src/core/str/mod.rs.html#193
pub const fn is_char_boundary(b: u8) -> bool {
    (b as i8) >= -0x40
}

#[derive(Default)]
pub struct ResidualStrBytes {
    bytes: [u8; std::mem::size_of::<char>()],
    len: u8,
}
impl ResidualStrBytes {
    pub fn receive_bytes<'a>(
        &mut self,
        buf: &'a mut [u8; std::mem::size_of::<char>()],
        mut bytes: &'a [u8],
    ) -> [&'a str; 2] {
        loop {
            if bytes.is_empty() {
                break;
            }

            let b = bytes[0];
            if is_char_boundary(b) {
                break;
            }

            if self.len == self.bytes.len() as _ {
                self.len = 0;
                break;
            }

            self.bytes[self.len as usize] = bytes[0];
            self.len += 1;
            bytes = &bytes[1..];
        }

        *buf = self.bytes;
        let before = &buf[..self.len as usize];
        self.len = 0;

        let mut len = bytes.len();
        loop {
            if len == 0 {
                break;
            }
            len -= 1;
            if is_char_boundary(bytes[len]) {
                break;
            }
        }

        let (after, rest) = bytes.split_at(len);
        if self.bytes.len() < rest.len() {
            return ["", ""];
        }

        self.len = rest.len() as _;
        self.bytes[..self.len as usize].copy_from_slice(rest);

        let before = std::str::from_utf8(before).unwrap_or("");
        let after = std::str::from_utf8(after).unwrap_or("");

        [before, after]
    }
}

pub fn parse_process_command(command: &str) -> Option<Command> {
    let mut tokenizer = CommandTokenizer(command);
    let name = tokenizer.next()?;
    let mut command = Command::new(name);
    for arg in tokenizer {
        command.arg(arg);
    }
    Some(command)
}

pub fn load_config(ctx: &mut EditorContext, config_name: &str, config_content: &str) -> EditorFlow {
    for (line_index, line) in config_content.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut command = ctx.editor.string_pool.acquire_with(line);
        let result = CommandManager::try_eval(ctx, None, &mut command);
        ctx.editor.string_pool.release(command);

        match result {
            Ok(flow) => match flow {
                EditorFlow::Continue => (),
                _ => return flow,
            },
            Err(error) => {
                ctx.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!(
                        "{}:{}\n{}\n{}",
                        config_name,
                        line_index + 1,
                        line,
                        error
                    ));
                break;
            }
        }
    }

    EditorFlow::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_char_boundary_test() {
        let bytes = "áé".as_bytes();
        assert_eq!(4, bytes.len());
        assert!(is_char_boundary(bytes[0]));
        assert!(!is_char_boundary(bytes[1]));
        assert!(is_char_boundary(bytes[2]));
        assert!(!is_char_boundary(bytes[3]));
    }

    #[test]
    fn residual_str_bytes() {
        let message = "abcdef".as_bytes();
        let mut residue = ResidualStrBytes::default();
        assert_eq!(
            ["", "ab"],
            residue.receive_bytes(&mut Default::default(), &message[..3])
        );
        assert_eq!(
            ["c", "de"],
            residue.receive_bytes(&mut Default::default(), &message[3..])
        );
        assert_eq!(
            ["f", ""],
            residue.receive_bytes(&mut Default::default(), &message[6..])
        );
        assert_eq!(
            ["", ""],
            residue.receive_bytes(&mut Default::default(), &[])
        );

        let message1 = "abcdef".as_bytes();
        let message2 = "123456".as_bytes();
        let mut residue = ResidualStrBytes::default();
        assert_eq!(
            ["", "abcde"],
            residue.receive_bytes(&mut Default::default(), &message1)
        );
        assert_eq!(
            ["f", "12345"],
            residue.receive_bytes(&mut Default::default(), &message2)
        );
        assert_eq!(
            ["6", ""],
            residue.receive_bytes(&mut Default::default(), &[])
        );
        assert_eq!(
            ["", ""],
            residue.receive_bytes(&mut Default::default(), &[])
        );

        let message = "áéíóú".as_bytes();
        assert_eq!(10, message.len());
        let mut residue = ResidualStrBytes::default();
        assert_eq!(
            ["", "á"],
            residue.receive_bytes(&mut Default::default(), &message[..3])
        );
        assert_eq!(
            ["é", ""],
            residue.receive_bytes(&mut Default::default(), &message[3..5])
        );
        assert_eq!(
            ["í", ""],
            residue.receive_bytes(&mut Default::default(), &message[5..8])
        );
        assert_eq!(
            ["ó", ""],
            residue.receive_bytes(&mut Default::default(), &message[8..])
        );
        assert_eq!(
            ["ú", ""],
            residue.receive_bytes(&mut Default::default(), &message[10..])
        );
        assert_eq!(
            ["", ""],
            residue.receive_bytes(&mut Default::default(), &[])
        );
    }
}

