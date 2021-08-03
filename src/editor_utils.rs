use std::{fmt, process::Command};

use crate::{
    client::ClientManager,
    command::{CommandManager, CommandTokenizer},
    editor::{BufferedKeys, Editor, EditorControlFlow, KeysIterator},
    platform::{Key, Platform},
    word_database::{WordIter, WordKind},
};

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

    pub fn write(&mut self, kind: MessageKind) -> EditorOutputWrite {
        self.kind = kind;
        self.message.clear();
        EditorOutputWrite(&mut self.message)
    }
}
pub struct EditorOutputWrite<'a>(&'a mut String);
impl<'a> EditorOutputWrite<'a> {
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

// FNV-1a : https://en.wikipedia.org/wiki/Fowler–Noll–Vo_hash_function
// TODO: is it still a good hash if we hash 8 bytes at a time and then combine them at the end?
// or should we just jump directly to a more complex hash that is simd-friendly?
pub const fn hash_bytes(mut bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    while let [b, rest @ ..] = bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        bytes = rest;
    }
    hash
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

pub fn load_config(
    editor: &mut Editor,
    platform: &mut Platform,
    clients: &mut ClientManager,
    config_name: &str,
    config_content: &str,
) -> EditorControlFlow {
    for (line_index, line) in config_content.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut command = editor.string_pool.acquire_with(line);
        let result = CommandManager::try_eval(editor, platform, clients, None, &mut command);
        editor.string_pool.release(command);

        match result {
            Ok(flow) => match flow {
                EditorControlFlow::Continue => (),
                _ => return flow,
            },
            Err(error) => {
                editor
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

    EditorControlFlow::Continue
}
