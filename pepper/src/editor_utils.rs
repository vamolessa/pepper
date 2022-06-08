use std::{fmt, fs, io, process::Command};

use crate::{
    buffer::char_display_len,
    command::CommandTokenizer,
    editor::{BufferedKeys, KeysIterator},
    events::{KeyParseAllError, KeyParser},
    mode::ModeKind,
    picker::Picker,
    platform::{Key, KeyCode, Platform},
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
            Key {
                code: KeyCode::Esc,
                control: false,
                alt: false,
                ..
            }
            | Key {
                code: KeyCode::Char('c'),
                shift: false,
                control: true,
                alt: false,
            } => ReadLinePoll::Canceled,
            Key {
                code: KeyCode::Char('\n'),
                control: false,
                alt: false,
                ..
            }
            | Key {
                code: KeyCode::Char('m'),
                shift: false,
                control: true,
                alt: false,
            } => ReadLinePoll::Submitted,
            Key {
                code: KeyCode::Home,
                shift: false,
                control: false,
                alt: false,
            }
            | Key {
                code: KeyCode::Char('u'),
                shift: false,
                control: true,
                alt: false,
            } => {
                self.input.clear();
                ReadLinePoll::Pending
            }
            Key {
                code: KeyCode::Char('w'),
                shift: false,
                control: true,
                alt: false,
            } => {
                let mut words = WordIter(&self.input);
                (&mut words)
                    .filter(|w| w.kind == WordKind::Identifier)
                    .next_back();
                let len = words.0.len();
                self.input.truncate(len);
                ReadLinePoll::Pending
            }
            Key {
                code: KeyCode::Backspace,
                shift: false,
                control: false,
                alt: false,
            }
            | Key {
                code: KeyCode::Char('h'),
                shift: false,
                control: true,
                alt: false,
            } => {
                if let Some((last_char_index, _)) = self.input.char_indices().next_back() {
                    self.input.truncate(last_char_index);
                }
                ReadLinePoll::Pending
            }
            Key {
                code: KeyCode::Char('y'),
                shift: false,
                control: true,
                alt: false,
            } => {
                let mut text = string_pool.acquire();
                platform.read_from_clipboard(&mut text);
                self.input.push_str(&text);
                string_pool.release(text);
                ReadLinePoll::Pending
            }
            Key {
                code: KeyCode::Char('\t'),
                control: false,
                alt: false,
                ..
            } => {
                self.input.push(' ');
                ReadLinePoll::Pending
            }
            Key {
                code: KeyCode::Char(c),
                control: false,
                alt: false,
                ..
            } => {
                self.input.push(c);
                ReadLinePoll::Pending
            }
            _ => ReadLinePoll::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MessageKind {
    Status,
    Info,
    Error,
}

#[derive(Default)]
pub struct LoggerStatusBarDisplay<'logger, 'lines> {
    pub prefix: &'static str,
    pub prefix_is_line: bool,
    pub lines: &'lines [&'logger str],
}

pub struct Logger {
    kind: MessageKind,
    message: String,
    log_file_path: String,
    log_file: Option<fs::File>,
}
impl Logger {
    pub fn new(log_file_path: String, log_file: Option<fs::File>) -> Self {
        Self {
            kind: MessageKind::Info,
            message: String::new(),
            log_file_path,
            log_file,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.message.is_empty()
    }

    pub fn clear(&mut self) {
        self.message.clear();
    }

    pub fn write(&mut self, kind: MessageKind) -> LogWriter {
        self.kind = kind;
        self.message.clear();
        LogWriter(self)
    }

    pub(crate) fn on_before_render(&mut self) {
        let trimmed_len = self.message.trim_end().len();
        self.message.truncate(trimmed_len);

        unsafe {
            for b in self.message.as_mut_vec().iter_mut() {
                if *b == b'\t' {
                    *b = b' ';
                }
            }
        }
    }

    pub(crate) fn display_to_status_bar<'this, 'lines>(
        &'this self,
        available_size: (u16, u8),
        lines: &'lines mut [&'this str],
    ) -> LoggerStatusBarDisplay<'this, 'lines> {
        let lines = if lines.len() > available_size.1 as _ {
            &mut lines[..available_size.1 as usize]
        } else {
            lines
        };

        let prefix = match self.kind {
            MessageKind::Status | MessageKind::Info => "",
            MessageKind::Error => "error:",
        };

        let mut lines_len = 0;
        let mut x = 0;
        let mut line_start_index = 0;
        let mut prefix_is_line = false;

        if (prefix.len() + self.message.len()) < available_size.0 as _ {
            x = prefix.len();
        } else {
            prefix_is_line = !prefix.is_empty();
        }

        for (i, c) in self.message.char_indices() {
            match c {
                '\n' => {
                    if lines_len >= lines.len() {
                        break;
                    }

                    lines[lines_len] = &self.message[line_start_index..i];
                    lines_len += 1;
                    line_start_index = i + 1;
                }
                c => {
                    let c_len = char_display_len(c) as usize;
                    x += c_len;
                    if x >= available_size.0 as _ {
                        x = c_len;

                        if lines_len >= lines.len() {
                            break;
                        }

                        lines[lines_len] = &self.message[line_start_index..i];
                        lines_len += 1;
                        line_start_index = i;
                    }
                }
            }
        }
        if lines_len < lines.len() && line_start_index < self.message.len() {
            lines[lines_len] = &self.message[line_start_index..];
            lines_len += 1;
        }

        LoggerStatusBarDisplay {
            prefix,
            prefix_is_line,
            lines: &lines[..lines_len],
        }
    }

    pub fn log_file_path(&self) -> Option<&str> {
        if self.log_file.is_some() {
            Some(&self.log_file_path)
        } else {
            None
        }
    }
}
impl Drop for Logger {
    fn drop(&mut self) {
        if self.log_file.is_some() {
            let _ = fs::remove_file(&self.log_file_path);
        }
    }
}

pub struct LogWriter<'a>(&'a mut Logger);
impl<'a> LogWriter<'a> {
    pub fn str(&mut self, message: &str) {
        self.0.message.push_str(message);
    }

    pub fn fmt(&mut self, args: fmt::Arguments) {
        let _ = fmt::write(&mut self.0.message, args);
    }
}
impl<'a> Drop for LogWriter<'a> {
    fn drop(&mut self) {
        if let MessageKind::Error = self.0.kind {
            if let Some(log_file) = &mut self.0.log_file {
                use io::Write;
                self.0.message.push('\n');
                let _ = log_file.write_all(self.0.message.as_bytes());
                self.0.message.truncate(self.0.message.len() - 1);
            }
        }
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

    pub fn from_str(key: &str) -> Option<Self> {
        let mut chars = key.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        Self::from_char(c)
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

#[derive(Default)]
pub(crate) struct PickerEntriesProcessBuf {
    buf: Vec<u8>,
    waiting_for_process: bool,
}
impl PickerEntriesProcessBuf {
    pub(crate) fn on_process_spawned(&mut self) {
        self.waiting_for_process = true;
    }

    pub(crate) fn on_process_output(
        &mut self,
        picker: &mut Picker,
        read_line: &ReadLine,
        bytes: &[u8],
    ) {
        if !self.waiting_for_process {
            return;
        }

        self.buf.extend_from_slice(bytes);

        {
            let mut entry_adder = picker.add_custom_filtered_entries(read_line.input());
            if let Some(i) = self.buf.iter().rposition(|&b| b == b'\n') {
                for line in self
                    .buf
                    .drain(..i + 1)
                    .as_slice()
                    .split(|&b| matches!(b, b'\n' | b'\r'))
                {
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(line) = std::str::from_utf8(line) {
                        entry_adder.add(line);
                    }
                }
            }
        }

        picker.move_cursor(0);
    }

    pub(crate) fn on_process_exit(&mut self, picker: &mut Picker, read_line: &ReadLine) {
        if !self.waiting_for_process {
            return;
        }

        self.waiting_for_process = false;

        {
            let mut entry_adder = picker.add_custom_filtered_entries(read_line.input());
            for line in self.buf.split(|&b| b == b'\n') {
                if line.is_empty() {
                    continue;
                }
                if let Ok(line) = std::str::from_utf8(line) {
                    entry_adder.add(line);
                }
            }
        }

        self.buf.clear();
        picker.move_cursor(0);
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
    let mut tokens = CommandTokenizer(command);
    let name = tokens.next()?.slice;
    let mut command = Command::new(name);
    for arg in tokens {
        command.arg(arg.slice);
    }
    Some(command)
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
