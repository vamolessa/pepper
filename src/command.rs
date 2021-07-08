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

mod builtin;

pub const HISTORY_CAPACITY: usize = 10;

pub enum CommandError {
    NoSuchCommand,
    TooManyArguments,
    TooFewArguments,
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
        match self.try_next_arg()? {
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
        if self.args.bang || !self.editor.buffers.iter().any(Buffer::needs_save) {
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
            .ok_or(CommandError::InvalidBufferHandle(handle))?;
        if self.args.bang || !buffer.needs_save() {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }
}

struct CommandIter<'a>(pub &'a str);
impl<'a> Iterator for CommandIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.0 = self.0.trim_start();
            if self.0.is_empty() {
                return None;
            }

            let bytes = self.0.as_bytes();
            let mut i = 0;

            loop {
                if i == bytes.len() {
                    let command = self.0;
                    self.0 = "";
                    return Some(command);
                }

                match bytes[i] {
                    b'\n' | b';' => {
                        let command = &self.0[..i];
                        self.0 = &self.0[i + 1..];
                        if command.is_empty() {
                            break;
                        } else {
                            return Some(command);
                        }
                    }
                    b'{' => match find_balanced(&bytes[i + 1..], b'{', b'}') {
                        Some(len) => i += len + 1,
                        None => {
                            let command = self.0;
                            self.0 = "";
                            return Some(command);
                        }
                    },
                    b'#' => {
                        let command = &self.0[..i];
                        while i < bytes.len() && bytes[i] != b'\n' {
                            i += 1;
                        }
                        self.0 = &self.0[i..];
                        if command.is_empty() {
                            break;
                        } else {
                            return Some(command);
                        }
                    }
                    _ => (),
                }

                i += 1;
            }
        }
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
            builtin_commands: builtin::COMMANDS,
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
            args,
        };
        let args = CommandArgs(tokenizer);
        (command.func)(&mut ctx, args, bang)

        Ok(None)
    }

    pub fn eval_and_then_output<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        commands: &'command str,
        source_path: Option<&'command Path>,
    ) -> Option<CommandOperation> {
        let mut output = editor.string_pool.acquire();

        let operation = match Self::eval(
            editor,
            platform,
            clients,
            client_handle,
            commands,
            source_path,
            &mut output,
        ) {
            Ok(op) => op,
            Err((command, error)) => {
                output.clear();
                let error = error.display(command, source_path, &editor.commands, &editor.buffers);
                editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error));
                None
            }
        };

        match client_handle
            .and_then(|h| clients.get(h))
            .filter(|c| !c.has_ui())
            .map(Client::handle)
        {
            Some(handle) => {
                let mut buf = platform.buf_pool.acquire();
                ServerEvent::CommandOutput(&output).serialize(buf.write());
                let buf = buf.share();

                platform.buf_pool.release(buf.clone());
                platform.enqueue_request(PlatformRequest::WriteToClient { handle, buf });
            }
            None => {
                if !output.is_empty() {
                    editor.status_bar.write(MessageKind::Info).str(&output)
                }
            }
        }

        editor.string_pool.release(output);
        operation
    }

    pub fn eval<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        commands: &'command str,
        source_path: Option<&'command Path>,
        output: &mut String,
    ) -> Result<Option<CommandOperation>, (&'command str, CommandError)> {
        for command in CommandIter(commands) {
            let op = Self::eval_single_command(
                editor,
                platform,
                clients,
                client_handle,
                command,
                source_path,
                output,
            );
            editor.trigger_event_handlers(platform, clients);
            match op {
                Ok(Some(op)) => return Ok(Some(op)),
                Ok(None) => (),
                Err(error) => return Err((command, error)),
            }
        }
        Ok(None)
    }

    fn eval_single_command<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &'command str,
        source_path: Option<&'command Path>,
        output: &mut String,
    ) -> Result<Option<CommandOperation>, CommandError> {
        output.clear();
        let ParsedStatement {
            target_register,
            expression,
        } = editor.commands.parse(command)?;

        let result = match expression {
            ParsedExpression::Literal(value) => {
                output.push_str(value);
                Ok(None)
            }
            ParsedExpression::Register(register) => {
                output.push_str(editor.registers.get(register));
                Ok(None)
            }
            ParsedExpression::Command {
                source: CommandSource::Builtin(i),
                args,
            } => {
                let command = editor.commands.builtin_commands[i].func;
                let mut ctx = CommandContext {
                    editor,
                    platform,
                    clients,
                    client_handle,
                    source_path,
                    args,
                    output,
                };
                command(&mut ctx)
            }
            ParsedExpression::Command {
                source: CommandSource::Macro(i),
                args,
            } => {
                assert_no_bang(args.bang)?;
                let mut tokens = args.tokens.clone();
                get_flags(tokens.clone(), &editor.registers, &mut [])?;

                let macro_command = &editor.commands.macro_commands[i];
                let body = editor.string_pool.acquire_with(&macro_command.body);

                let mut arg_count = 0;
                for &key in &macro_command.params {
                    arg_count += 1;
                    match try_next_raw_value(&mut tokens)? {
                        Some(RawCommandValue::Literal(token)) => {
                            editor.registers.set(key, token.as_str(tokens.raw))
                        }
                        Some(RawCommandValue::Register(_, register)) => {
                            editor.registers.copy(register, key)
                        }
                        None => return Err(CommandError::TooFewArguments(arg_count)),
                    }
                }
                assert_empty(&mut tokens, macro_command.params.len() as _)?;

                let result = match Self::eval(
                    editor,
                    platform,
                    clients,
                    client_handle,
                    &body,
                    source_path,
                    output,
                ) {
                    Ok(op) => Ok(op),
                    Err((command, error)) => Err(CommandError::MacroCommandError {
                        index: i,
                        command: command.into(),
                        error: Box::new(error),
                    }),
                };

                editor.string_pool.release(body);
                result
            }
            ParsedExpression::Command {
                source: CommandSource::Request(i),
                args,
            } => {
                let args = args.with(&editor.registers);
                args.assert_no_bang()?;

                let handle = editor.commands.request_commands[i].client_handle;

                let mut buf = platform.buf_pool.acquire();
                let write = buf.write();
                ServerEvent::Request(command).serialize(write);
                let buf = buf.share();
                platform.enqueue_request(PlatformRequest::WriteToClient { handle, buf });

                Ok(None)
            }
        };

        if let Some(register) = target_register {
            editor.registers.set(register, output);
            output.clear();
        }
        result
    }

    pub fn spawn_process(
        &mut self,
        platform: &mut Platform,
        client_handle: Option<ClientHandle>,
        mut command: Command,
        stdin: Option<&str>,
        on_output: Option<&str>,
        split_on_byte: Option<u8>,
    ) {
        let mut index = None;
        for (i, process) in self.spawned_processes.iter().enumerate() {
            if !process.alive {
                index = Some(i);
                break;
            }
        }
        let index = match index {
            Some(index) => index,
            None => {
                let index = self.spawned_processes.len();
                self.spawned_processes.push(Default::default());
                index
            }
        };

        let process = &mut self.spawned_processes[index];
        process.alive = true;
        process.client_handle = client_handle;
        process.output.clear();
        process.split_on_byte = split_on_byte;
        process.on_output.clear();

        match stdin {
            Some(stdin) => {
                let mut buf = platform.buf_pool.acquire();
                let writer = buf.write();
                writer.extend_from_slice(stdin.as_bytes());
                let buf = buf.share();
                platform.buf_pool.release(buf.clone());

                command.stdin(Stdio::piped());
                process.input = Some(buf);
            }
            None => {
                command.stdin(Stdio::null());
                process.input = None;
            }
        }
        match on_output {
            Some(on_output) => {
                command.stdout(Stdio::piped());
                process.on_output.push_str(on_output);
            }
            None => {
                command.stdout(Stdio::null());
            }
        }
        command.stderr(Stdio::null());

        platform.enqueue_request(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Command(index),
            command,
            buf_len: if on_output.is_some() { 4 * 1024 } else { 0 },
        });
    }

    pub fn on_process_spawned(
        &mut self,
        platform: &mut Platform,
        index: usize,
        handle: ProcessHandle,
    ) {
        if let Some(buf) = self.spawned_processes[index].input.take() {
            platform.enqueue_request(PlatformRequest::WriteToProcess { handle, buf });
            platform.enqueue_request(PlatformRequest::CloseProcessInput { handle });
        }
    }

    pub fn on_process_output(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        index: usize,
        bytes: &[u8],
    ) {
        let process = &mut editor.commands.spawned_processes[index];
        if process.on_output.is_empty() {
            return;
        }
        process.output.extend_from_slice(bytes);
        let split_on_byte = match process.split_on_byte {
            Some(b) => b,
            None => return,
        };

        let client_handle = process.client_handle;
        let commands = editor.string_pool.acquire_with(&process.on_output);
        let mut output_index = 0;

        loop {
            let process = &editor.commands.spawned_processes[index];
            let stdout = &process.output[output_index..];
            let slice = match stdout.iter().position(|&b| b == split_on_byte) {
                Some(i) => {
                    output_index += i + 1;
                    &stdout[..i]
                }
                None => break,
            };

            if slice.is_empty() {
                continue;
            }

            match std::str::from_utf8(slice) {
                Ok(slice) => {
                    editor.registers.set(RETURN_REGISTER, slice);
                    Self::eval_and_then_output(
                        editor,
                        platform,
                        clients,
                        client_handle,
                        &commands,
                        None,
                    );
                }
                Err(error) => {
                    editor
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!("{}", error));
                }
            }
        }

        editor.string_pool.release(commands);
        editor.commands.spawned_processes[index]
            .output
            .drain(..output_index);
    }

    pub fn on_process_exit(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        index: usize,
    ) {
        let process = &mut editor.commands.spawned_processes[index];
        process.alive = false;
        if process.on_output.is_empty() {
            return;
        }
        if process.output.is_empty() && process.split_on_byte.is_some() {
            return;
        }

        match std::str::from_utf8(&process.output) {
            Ok(stdout) => editor.registers.set(RETURN_REGISTER, stdout),
            Err(error) => {
                editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error));
                return;
            }
        }

        let client_handle = process.client_handle;
        let commands = editor.string_pool.acquire_with(&process.on_output);
        Self::eval_and_then_output(editor, platform, clients, client_handle, &commands, None);
        editor.string_pool.release(commands);
    }

    fn parse<'a>(&self, raw: &'a str) -> Result<ParsedStatement<'a>, CommandError> {
        let mut tokens = CommandTokenIter::new(raw);

        let mut target_register = None;
        let (command_token, command_name) = loop {
            match tokens.next() {
                Some((CommandTokenKind::Identifier, token)) => break (token, token.as_str(raw)),
                Some((CommandTokenKind::String, token)) => match tokens.next() {
                    Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                    None => {
                        return Ok(ParsedStatement {
                            target_register,
                            expression: ParsedExpression::Literal(token.as_str(raw)),
                        })
                    }
                },
                Some((CommandTokenKind::Register, token)) => {
                    let register = parse_register_key(raw, token)?;
                    match target_register {
                        Some(_) => match tokens.next() {
                            Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                            None => {
                                return Ok(ParsedStatement {
                                    target_register,
                                    expression: ParsedExpression::Register(register),
                                })
                            }
                        },
                        None => match tokens.next() {
                            Some((CommandTokenKind::Equals, _)) => target_register = Some(register),
                            Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                            None => {
                                return Ok(ParsedStatement {
                                    target_register,
                                    expression: ParsedExpression::Register(register),
                                })
                            }
                        },
                    }
                }
                Some((_, token)) => return Err(CommandError::InvalidCommandName(token)),
                None => return Err(CommandError::InvalidCommandName(tokens.end_token())),
            }
        };

        let (command_name, bang) = match command_name.strip_suffix('!') {
            Some(command_name) => (command_name, true),
            None => (command_name, false),
        };
        if command_name.is_empty() {
            return Err(CommandError::InvalidCommandName(command_token));
        }

        let source = match self.find_command(command_name) {
            Some(source) => source,
            None => return Err(CommandError::CommandNotFound(command_token)),
        };
        Ok(ParsedStatement {
            target_register,
            expression: ParsedExpression::Command {
                source,
                args: CommandArgsBuilder { tokens, bang },
            },
        })
    }
}

pub fn parse_process_command(
    registers: &RegisterCollection,
    command: &str,
    environment: &str,
) -> Result<Command, CommandError> {
    let mut command_tokens = CommandTokenIter::new(command);
    let command_name = match command_tokens.next() {
        Some((
            CommandTokenKind::Identifier
            | CommandTokenKind::String
            | CommandTokenKind::Flag
            | CommandTokenKind::Equals,
            token,
        )) => token.as_str(command),
        Some((CommandTokenKind::Register, token)) => {
            let register = parse_register_key(command, token)?;
            registers.get(register)
        }
        Some((CommandTokenKind::Unterminated, token)) => {
            return Err(CommandError::UnterminatedToken(token))
        }
        None => return Err(CommandError::InvalidToken(command_tokens.end_token())),
    };

    let mut process_command = Command::new(command_name);
    while let Some((kind, token)) = command_tokens.next() {
        let arg = match kind {
            CommandTokenKind::Identifier
            | CommandTokenKind::String
            | CommandTokenKind::Flag
            | CommandTokenKind::Equals => token.as_str(command),
            CommandTokenKind::Register => {
                let register = parse_register_key(command, token)?;
                registers.get(register)
            }
            CommandTokenKind::Unterminated => return Err(CommandError::InvalidToken(token)),
        };
        process_command.arg(arg);
    }

    let mut environment_tokens = CommandTokenIter::new(environment);
    loop {
        let key = match environment_tokens.next() {
            Some((CommandTokenKind::Identifier | CommandTokenKind::String, token)) => {
                token.as_str(environment)
            }
            Some((CommandTokenKind::Register, token)) => {
                let register = parse_register_key(environment, token)?;
                registers.get(register)
            }
            Some((CommandTokenKind::Flag | CommandTokenKind::Equals, token)) => {
                return Err(CommandError::InvalidToken(token))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                return Err(CommandError::UnterminatedToken(token))
            }
            None => break,
        };
        match environment_tokens.next() {
            Some((CommandTokenKind::Equals, _)) => (),
            Some((_, token)) => return Err(CommandError::InvalidToken(token)),
            None => {
                return Err(CommandError::UnterminatedToken(
                    environment_tokens.end_token(),
                ))
            }
        }
        let value = match environment_tokens.next() {
            Some((CommandTokenKind::Identifier | CommandTokenKind::String, token)) => {
                token.as_str(environment)
            }
            Some((CommandTokenKind::Register, token)) => {
                let register = parse_register_key(environment, token)?;
                registers.get(register)
            }
            Some((CommandTokenKind::Flag | CommandTokenKind::Equals, token)) => {
                return Err(CommandError::InvalidToken(token))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                return Err(CommandError::UnterminatedToken(token))
            }
            None => {
                return Err(CommandError::UnterminatedToken(
                    environment_tokens.end_token(),
                ))
            }
        };

        process_command.env(key, value);
    }

    Ok(process_command)
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
