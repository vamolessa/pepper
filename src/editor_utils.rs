use std::fmt;

use crate::{
    config::ParseConfigError,
    editor::{BufferedKeys, Editor, KeysIterator},
    ini::{Ini, PropertyIterator},
    keymap::{KeyMapCollection, ParseKeyMapError},
    mode::ModeKind,
    platform::{Key, Platform},
    register::RegisterCollection,
    syntax::Syntax,
    theme::Color,
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
        registers: &RegisterCollection,
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
                if let Some((last_char_index, _)) = self.input.char_indices().rev().next() {
                    self.input.truncate(last_char_index);
                }
                ReadLinePoll::Pending
            }
            Key::Ctrl('y') => {
                let mut text = string_pool.acquire();
                platform.read_from_clipboard(registers, &mut text);
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
    return hash;
}

pub fn load_config<'content>(
    editor: &mut Editor,
    ini: &mut Ini<'content>,
    config_name: &str,
    config_content: &'content str,
) {
    fn parse_bindings(
        keymaps: &mut KeyMapCollection,
        mode: ModeKind,
        bindings: PropertyIterator,
        config_name: &str,
        output: &mut EditorOutputWrite,
    ) {
        for (key, value, line_index) in bindings {
            match keymaps.parse_and_map(mode, key, value) {
                Ok(()) => (),
                Err(ParseKeyMapError::From(error)) => output.fmt(format_args!(
                    "from binding parse error '{}' at {}:{}",
                    error, config_name, line_index,
                )),
                Err(ParseKeyMapError::To(error)) => output.fmt(format_args!(
                    "to binding parse error '{}' at {}:{}",
                    error, config_name, line_index,
                )),
            }
        }
    }

    let mut output = editor.status_bar.write(MessageKind::Error);

    if let Err(error) = ini.parse(config_content) {
        output.fmt(format_args!(
            "error parsing config {}:{} : {}",
            config_name, error.line_index, error.kind,
        ));
        return;
    }

    'section_loop: for (section, line_index, properties) in ini.sections() {
        match section {
            "config" => {
                for (key, value, line_index) in properties {
                    match editor.config.parse_config(key, value) {
                        Ok(()) => (),
                        Err(ParseConfigError::NoSuchConfig) => output.fmt(format_args!(
                            "no such config '{}' at {}:{}\n",
                            key, config_name, line_index
                        )),
                        Err(ParseConfigError::InvalidValue) => output.fmt(format_args!(
                            "invalid config value '{}' at {}:{}\n",
                            value, config_name, line_index,
                        )),
                    }
                }
            }
            "theme" => {
                for (key, value, line_index) in properties {
                    let color = match editor.theme.color_from_name(key) {
                        Some(color) => color,
                        None => {
                            output.fmt(format_args!(
                                "no such color '{}' at {}:{}\n",
                                key, config_name, line_index
                            ));
                            continue;
                        }
                    };
                    let encoded = match u32::from_str_radix(value, 16) {
                        Ok(value) => value,
                        Err(_) => {
                            output.fmt(format_args!(
                                "invalid color value '{}' at {}:{}\n",
                                value, config_name, line_index
                            ));
                            continue;
                        }
                    };
                    *color = Color::from_u32(encoded);
                }
            }
            "syntax" => {
                let mut syntax = Syntax::new();
                let mut has_glob = false;
                for (key, value, line_index) in properties {
                    match key {
                        "glob" => match syntax.set_glob(value) {
                            Ok(()) => has_glob = true,
                            Err(_) => {
                                output.fmt(format_args!(
                                    "invalid glob '{}' at {}:{}",
                                    value, config_name, line_index,
                                ));
                                continue 'section_loop;
                            }
                        },
                        _ => match key.parse() {
                            Ok(kind) => match syntax.set_rule(kind, value) {
                                Ok(()) => (),
                                Err(error) => {
                                    output.fmt(format_args!(
                                        "syntax pattern error '{}' at {}:{}",
                                        error, config_name, line_index,
                                    ));
                                    continue 'section_loop;
                                }
                            },
                            Err(_) => {
                                output.fmt(format_args!(
                                    "no such token kind '{}' at {}:{}",
                                    key, config_name, line_index
                                ));
                                continue 'section_loop;
                            }
                        },
                    }
                }

                if !has_glob {
                    output.fmt(format_args!(
                        "syntax has no glob property at {}:{}",
                        config_name, line_index,
                    ));
                    continue;
                }

                editor.syntaxes.add(syntax);
            }
            "alias" => {
                todo!();
            }
            "normal-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Normal,
                properties,
                config_name,
                &mut output,
            ),
            "insert-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Insert,
                properties,
                config_name,
                &mut output,
            ),
            "command-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Command,
                properties,
                config_name,
                &mut output,
            ),
            "readline-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::ReadLine,
                properties,
                config_name,
                &mut output,
            ),
            "picker-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Picker,
                properties,
                config_name,
                &mut output,
            ),
            _ => output.fmt(format_args!(
                "no such config '{}' at {}:{}\n",
                section, config_name, line_index,
            )),
        }
    }
}

