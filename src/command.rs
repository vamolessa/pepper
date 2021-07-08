use std::{
    collections::VecDeque,
    fmt, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    buffer::{Buffer, BufferCollection, BufferError, BufferHandle},
    buffer_view::BufferViewHandle,
    client::{Client, ClientHandle, ClientManager},
    editor::Editor,
    editor_utils::MessageKind,
    events::{KeyParseError, ServerEvent},
    pattern::PatternError,
    platform::{Platform, PlatformRequest, ProcessHandle, ProcessTag, SharedBuf},
    register::{RegisterCollection, RegisterKey, RETURN_REGISTER},
    serialization::Serialize,
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
        /*
        let buffer = self
            .editor
            .buffers
            .get(handle)
            .ok_or(CommandError::InvalidBufferHandle(handle))?;
        if self.args.bang || !buffer.needs_save() {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
        */
        todo!()
    }
}

#[derive(Clone)]
pub struct CommandTokenizer<'a>(&'a str);
impl<'a> Iterator for CommandTokenizer<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        fn next_literal(s: &str) -> (&str, &str) {
            match s.find(&[' ', '\t', '"', '\''][..]) {
                Some(i) => s.split_at(i),
                None => (s, ""),
            }
        }

        self.0 = self.0.trim_start_matches(&[' ', '\t'][..]);
        match self.0.chars().next()? {
            delim @ ('"' | '\'') => {
                let rest = &self.0[1..];
                match self.0.find(delim) {
                    Some(i) => {
                        let token = &rest[..i];
                        self.0 = &self.0[i + 1..];
                        Some(token)
                    }
                    None => {
                        let (token, rest) = next_literal(self.0);
                        self.0 = rest;
                        Some(token)
                    }
                }
            }
            _ => {
                let (token, rest) = next_literal(self.0);
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
    pub hidden: bool,
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
        if let Some(i) = self
            .builtin_commands
            .iter()
            .position(|c| c.name == name)
        {
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
        (command.func)(&mut ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static EMPTY_REGISTERS: RegisterCollection = RegisterCollection::new();

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            name: "command-name",
            alias: "c",
            hidden: false,
            completions: &[],
            func: |_| Ok(None),
        }];

        CommandManager {
            builtin_commands,
            macro_commands: Vec::new(),
            request_commands: Vec::new(),
            history: Default::default(),
            spawned_processes: Vec::new(),
        }
    }

    #[test]
    fn operation_size() {
        assert_eq!(1, std::mem::size_of::<CommandOperation>());
        assert_eq!(1, std::mem::size_of::<Option<CommandOperation>>());
    }

    #[test]
    fn command_tokens() {
        let command = "value -flag";
        let mut tokens = CommandTokenIter::new(command);
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Identifier, token)) if token.as_str(command) == "value",
        ));
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Flag, token)) if token.as_str(command) == "-flag",
        ));
        assert!(tokens.next().is_none());

        let command = "value --long-flag";
        let mut tokens = CommandTokenIter::new(command);
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Identifier, token)) if token.as_str(command) == "value",
        ));
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Flag, token)) if token.as_str(command) == "--long-flag",
        ));
        assert!(tokens.next().is_none());
    }

    #[test]
    fn command_parsing() {
        fn assert_bang(commands: &CommandManager, command: &str, expect_bang: bool) {
            let (source, args) = match commands.parse(command) {
                Ok(ParsedStatement {
                    expression: ParsedExpression::Command { source, args },
                    ..
                }) => (source, args),
                _ => panic!("command parse error at '{}'", command),
            };
            assert!(matches!(source, CommandSource::Builtin(0)));
            assert_eq!(expect_bang, args.bang);
        }

        let commands = create_commands();
        assert_bang(&commands, "command-name", false);
        assert_bang(&commands, "  command-name  ", false);
        assert_bang(&commands, "  command-name!  ", true);
        assert_bang(&commands, "  command-name!", true);
    }

    #[test]
    fn arg_parsing() {
        fn parse_args<'a>(commands: &CommandManager, command: &'a str) -> CommandArgs<'a> {
            match commands.parse(command) {
                Ok(ParsedStatement {
                    expression: ParsedExpression::Command { args, .. },
                    ..
                }) => args.with(&EMPTY_REGISTERS),
                _ => panic!("command '{}' parse error", command),
            }
        }

        fn collect<'a>(mut args: CommandArgs<'a>) -> Vec<&'a str> {
            let mut values = Vec::new();
            loop {
                match args.try_next() {
                    Ok(Some(arg)) => values.push(arg.text),
                    Ok(None) => break,
                    Err(error) => {
                        let discriminant = std::mem::discriminant(&error);
                        panic!("error parsing args {:?}", discriminant);
                    }
                }
            }
            values
        }

        let commands = create_commands();
        let args = parse_args(&commands, "c  aaa  bbb  ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  'aaa'  \"bbb\"  ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  \"aaa\"\"bbb\"ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  {aaa}{bbb}ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  {aaa}{{bb}b}ccc  ");
        assert_eq!(["aaa", "{bb}b", "ccc"], &collect(args)[..]);

        fn flag_value<'a>(
            flags: &[(&str, Option<CommandValue<'a>>)],
            index: usize,
        ) -> Option<&'a str> {
            flags[index].1.as_ref().map(|f| f.text)
        }

        let args = parse_args(&commands, "c -option=value aaa");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(None, flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa"], &collect(args)[..]);

        let args = parse_args(&commands, "c 'aaa' -option=value");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(None, flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa"], &collect(args)[..]);

        let args = parse_args(&commands, "c aaa -switch bbb -option=value ccc");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(Some("-switch"), flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);

        let args = parse_args(&commands, "c -switch -option=value aaa");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(Some("-switch"), flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa"], &collect(args)[..]);
    }

    #[test]
    fn command_parsing_fail() {
        let commands = create_commands();

        macro_rules! assert_fail {
            ($command:expr, $error_pattern:pat => $value:ident == $expect:expr) => {
                let command = $command;
                match commands.parse(command) {
                    Ok(_) => panic!("command parsed successfully"),
                    Err($error_pattern) => assert_eq!($expect, $value.as_str(command)),
                    Err(_) => panic!("other error occurred"),
                }
            };
        }

        assert_fail!("", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!("   ", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!(" !", CommandError::InvalidCommandName(s) => s == "!");
        assert_fail!("!  'aa'", CommandError::InvalidCommandName(s) => s == "!");
        assert_fail!("  a \"bb\"", CommandError::CommandNotFound(s) => s == "a");

        fn assert_unterminated(args: &str) {
            let args = CommandArgsBuilder {
                tokens: CommandTokenIter::new(args),
                bang: false,
            };
            let mut args = args.with(&EMPTY_REGISTERS);

            loop {
                match args.try_next() {
                    Ok(Some(_)) => (),
                    Ok(None) => panic!("no unterminated token"),
                    Err(CommandError::UnterminatedToken(_)) => return,
                    Err(_) => panic!("other error"),
                }
            }
        }

        assert_unterminated("0 1 'abc");
        assert_unterminated("0 1 '");
        assert_unterminated("0 1 \"'");
    }

    #[test]
    fn test_find_balanced() {
        assert_eq!(None, find_balanced(b"", b'{', b'}'));
        assert_eq!(Some(0), find_balanced(b"}", b'{', b'}'));
        assert_eq!(Some(2), find_balanced(b"  }}", b'{', b'}'));
        assert_eq!(Some(2), find_balanced(b"{}}", b'{', b'}'));
        assert_eq!(Some(4), find_balanced(b"{{}}}", b'{', b'}'));
    }

    #[test]
    fn multi_command_line_parsing() {
        let mut commands = CommandIter("command0\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0\n\n\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0 {\n still command0\n}\ncommand1");
        assert_eq!(Some("command0 {\n still command0\n}"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0 }}} {\n {\n still command0\n}\n}\ncommand1");
        assert_eq!(
            Some("command0 }}} {\n {\n still command0\n}\n}"),
            commands.next()
        );
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("   #command0");
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0 # command1");
        assert_eq!(Some("command0 "), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("    # command0\ncommand1");
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands =
            CommandIter("command0# comment\n\n# more comment\n\n# one more comment\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0;command1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter(";;  command0;   ;;command1   ;");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1   "), commands.next());
        assert_eq!(None, commands.next());
    }
}
