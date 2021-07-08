use std::{collections::VecDeque, fmt};

use crate::{
    buffer::{Buffer, BufferHandle},
    buffer_view::BufferViewHandle,
    client::{Client, ClientHandle, ClientManager},
    editor::Editor,
    editor_utils::MessageKind,
    platform::Platform,
};

//mod builtin;

pub const HISTORY_CAPACITY: usize = 10;

pub enum CommandError {
    NoSuchCommand,
    TooManyArguments,
    TooFewArguments,
    NoBufferOpened,
    UnsavedChanges,
}
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoSuchCommand => f.write_str("no such command"),
            Self::TooManyArguments => f.write_str("too many arguments"),
            Self::TooFewArguments => f.write_str("too few arguments"),
            Self::NoBufferOpened => f.write_str("no buffer opened"),
            Self::UnsavedChanges => f.write_str("unsaved changes"),
        }
    }
}

type CommandFn = fn(&mut CommandContext) -> Result<Option<CommandOperation>, CommandError>;

pub enum CommandOperation {
    Suspend,
    Quit,
    QuitAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Commands,
    Buffers,
    Files,
    Custom(&'static [&'static str]),
}

pub struct CommandContext<'state, 'command> {
    pub editor: &'state mut Editor,
    pub platform: &'state mut Platform,
    pub clients: &'state mut ClientManager,
    pub client_handle: Option<ClientHandle>,

    tokenizer: CommandTokenizer<'command>,
    pub bang: bool,
}
impl<'state, 'command> CommandContext<'state, 'command> {
    pub fn try_next_arg(&mut self) -> Option<&'command str> {
        self.tokenizer.next()
    }

    pub fn next_arg(&mut self) -> Result<&'command str, CommandError> {
        match self.try_next_arg() {
            Some(value) => Ok(value),
            None => Err(CommandError::TooFewArguments),
        }
    }

    pub fn assert_empty_args(&mut self) -> Result<(), CommandError> {
        match self.try_next_arg() {
            Some(_) => Err(CommandError::TooManyArguments),
            None => Ok(()),
        }
    }

    pub fn current_buffer_view_handle(&self) -> Result<BufferViewHandle, CommandError> {
        match self
            .client_handle
            .and_then(|h| self.clients.get(h))
            .and_then(Client::buffer_view_handle)
        {
            Some(handle) => Ok(handle),
            None => Err(CommandError::NoBufferOpened),
        }
    }

    pub fn current_buffer_handle(&self) -> Result<BufferHandle, CommandError> {
        let buffer_view_handle = self.current_buffer_view_handle()?;
        match self
            .editor
            .buffer_views
            .get(buffer_view_handle)
            .map(|v| v.buffer_handle)
        {
            Some(handle) => Ok(handle),
            None => Err(CommandError::NoBufferOpened),
        }
    }

    pub fn assert_can_discard_all_buffers(&self) -> Result<(), CommandError> {
        if self.bang || !self.editor.buffers.iter().any(Buffer::needs_save) {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }

    pub fn assert_can_discard_buffer(&self, handle: BufferHandle) -> Result<(), CommandError> {
        let buffer = self
            .editor
            .buffers
            .get(handle)
            .ok_or(CommandError::NoBufferOpened)?;
        if self.bang || !buffer.needs_save() {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }
}

#[derive(Clone)]
pub struct CommandTokenizer<'a>(pub &'a str);
impl<'a> Iterator for CommandTokenizer<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        fn next_literal_end(s: &str) -> usize {
            match s.find(&[' ', '\t', '"', '\''][..]) {
                Some(i) => i,
                None => s.len(),
            }
        }

        self.0 = self.0.trim_start_matches(&[' ', '\t'][..]);

        match self.0.chars().next()? {
            delim @ ('"' | '\'') => {
                let rest = &self.0[1..];
                match rest.find(delim) {
                    Some(i) => {
                        let token = &rest[..i];
                        self.0 = &rest[i + 1..];
                        Some(token)
                    }
                    None => {
                        let end = next_literal_end(rest);
                        let (token, rest) = self.0.split_at(end + 1);
                        self.0 = rest;
                        Some(token)
                    }
                }
            }
            _ => {
                let end = next_literal_end(self.0);
                let (token, rest) = self.0.split_at(end);
                self.0 = rest;
                Some(token)
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum CommandSource {
    Builtin(usize),
}

pub struct BuiltinCommand {
    pub name: &'static str,
    pub completions: &'static [CompletionSource],
    pub func: CommandFn,
}

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    history: VecDeque<String>,
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            //builtin_commands: builtin::COMMANDS,
            builtin_commands: &[],
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
        }
    }

    pub fn find_command(&self, name: &str) -> Option<CommandSource> {
        if let Some(i) = self.builtin_commands.iter().position(|c| c.name == name) {
            return Some(CommandSource::Builtin(i));
        }

        None
    }

    pub fn builtin_commands(&self) -> &[BuiltinCommand] {
        &self.builtin_commands
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn history_entry(&self, index: usize) -> &str {
        match self.history.get(index) {
            Some(e) => &e[..],
            None => "",
        }
    }

    pub fn add_to_history(&mut self, entry: &str) {
        if entry.is_empty() || entry.starts_with(|c: char| c.is_ascii_whitespace()) {
            return;
        }
        if let Some(back) = self.history.back() {
            if back == entry {
                return;
            }
        }

        let mut s = if self.history.len() == self.history.capacity() {
            self.history.pop_front().unwrap()
        } else {
            String::new()
        };

        s.clear();
        s.push_str(entry);
        self.history.push_back(s);
    }

    pub fn eval(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> Option<CommandOperation> {
        match Self::try_eval(editor, platform, clients, client_handle, command) {
            Ok(op) => op,
            Err(error) => {
                editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error));
                None
            }
        }
    }

    pub fn try_eval(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> Result<Option<CommandOperation>, CommandError> {
        let mut tokenizer = CommandTokenizer(command);
        let command = match tokenizer.next() {
            Some(command) => command,
            None => return Err(CommandError::NoSuchCommand),
        };
        let (command, bang) = match command.strip_suffix('!') {
            Some(command) => (command, true),
            None => (command, false),
        };
        let command = match editor.commands.find_command(command) {
            Some(CommandSource::Builtin(i)) => &editor.commands.builtin_commands[i],
            None => return Err(CommandError::NoSuchCommand),
        };

        let mut ctx = CommandContext {
            editor,
            platform,
            clients,
            client_handle,
            tokenizer,
            bang,
        };
        let result = (command.func)(&mut ctx);
        editor.trigger_event_handlers(platform, clients);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_size() {
        assert_eq!(1, std::mem::size_of::<CommandOperation>());
        assert_eq!(1, std::mem::size_of::<Option<CommandOperation>>());
    }

    #[test]
    fn command_tokens() {
        let mut tokens = CommandTokenizer("cmd arg");
        assert_eq!(Some("cmd"), tokens.next());
        assert_eq!(Some("arg"), tokens.next());
        assert_eq!(None, tokens.next());
    
        let mut tokens = CommandTokenizer("cmd 'arg0 \"arg1 ");
        assert_eq!(Some("cmd"), tokens.next());
        assert_eq!(Some("'arg0"), tokens.next());
        assert_eq!(Some("\"arg1"), tokens.next());
        assert_eq!(None, tokens.next());
    
        let mut tokens = CommandTokenizer("cmd 'arg\"0' \"arg'1\"");
        assert_eq!(Some("cmd"), tokens.next());
        assert_eq!(Some("arg\"0"), tokens.next());
        assert_eq!(Some("arg'1"), tokens.next());
        assert_eq!(None, tokens.next());
    }
}

