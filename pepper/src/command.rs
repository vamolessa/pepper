use std::{collections::VecDeque, fmt, ops::Range};

use crate::{
    buffer::{Buffer, BufferHandle, BufferReadError, BufferWriteError},
    buffer_view::{BufferView, BufferViewHandle},
    client::ClientHandle,
    config::ParseConfigError,
    cursor::Cursor,
    editor::{EditorContext, EditorFlow},
    editor_utils::{LogKind, ParseKeyMapError},
    events::KeyParseAllError,
    glob::InvalidGlobError,
    pattern::PatternError,
    plugin::PluginHandle,
};

mod builtins;
mod expansions;

const HISTORY_CAPACITY: usize = 8;

pub enum CommandError {
    InvalidMacroName,
    ExpansionError(ExpansionError),
    NoSuchCommand,
    CommandArgsError(CommandArgsError),
    NoTargetClient,
    InvalidLogKind,
    EditorNotLogging,
    NoBufferOpened,
    UnsavedChanges,
    BufferReadError(BufferReadError),
    BufferWriteError(BufferWriteError),
    InvalidBufferPath,
    NoSuchBufferProperty,
    NoSuchBreakpointSubcommand,
    ConfigError(ParseConfigError),
    NoSuchColor,
    InvalidColorValue,
    InvalidModeKind,
    KeyMapError(ParseKeyMapError),
    KeyParseError(KeyParseAllError),
    InvalidRegisterKey,
    InvalidTokenKind,
    PatternError(PatternError),
    InvalidEnvironmentVariable,
    InvalidProcessCommand,
    InvalidIfOp,
    InvalidGlob(InvalidGlobError),
    OtherStatic(&'static str),
    OtherOwned(String),
}
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidMacroName => f.write_str("invalid command name"),
            Self::ExpansionError(error) => write!(f, "expansion error: {}", error),
            Self::NoSuchCommand => f.write_str("no such command"),
            Self::CommandArgsError(error) => write!(f, "args error: {}", error),
            Self::NoTargetClient => f.write_str("no target client"),
            Self::InvalidLogKind => f.write_str("invalid log kind"),
            Self::EditorNotLogging => f.write_str("editor is not logging"),
            Self::NoBufferOpened => f.write_str("no buffer opened"),
            Self::UnsavedChanges => f.write_str("unsaved changes"),
            Self::BufferReadError(error) => write!(f, "buffer read error: {}", error),
            Self::BufferWriteError(error) => write!(f, "buffer write error: {}", error),
            Self::InvalidBufferPath => f.write_str("invalid buffer path"),
            Self::NoSuchBufferProperty => f.write_str("no such buffer property"),
            Self::NoSuchBreakpointSubcommand => f.write_str("no such breakpoint subcommand"),
            Self::ConfigError(error) => write!(f, "config error: {}", error),
            Self::NoSuchColor => f.write_str("no such color"),
            Self::InvalidColorValue => f.write_str("invalid color value"),
            Self::InvalidModeKind => f.write_str("invalid mode"),
            Self::KeyMapError(error) => write!(f, "key map error: {}", error),
            Self::KeyParseError(error) => write!(f, "key parse error: {}", error),
            Self::InvalidRegisterKey => f.write_str("invalid register key"),
            Self::InvalidTokenKind => f.write_str("invalid token kind"),
            Self::PatternError(error) => write!(f, "pattern error: {}", error),
            Self::InvalidEnvironmentVariable => f.write_str("invalid environment variable"),
            Self::InvalidProcessCommand => f.write_str("invalid process command"),
            Self::InvalidIfOp => f.write_str("invalid if comparison operator"),
            Self::InvalidGlob(error) => write!(f, "glob error: {}", error),
            Self::OtherStatic(error) => f.write_str(error),
            Self::OtherOwned(error) => f.write_str(&error),
        }
    }
}
impl From<CommandArgsError> for CommandError {
    fn from(other: CommandArgsError) -> Self {
        Self::CommandArgsError(other)
    }
}

pub enum ExpansionError {
    NoSuchExpansion,
    IgnoreExpansion,
    CommandArgsError(CommandArgsError),
    InvalidArgIndex,
    InvalidBufferId,
    NoSuchCommand,
    InvalidCursorIndex,
    InvalidRegisterKey,
    OtherStatic(&'static str),
    OtherOwned(String),
}
impl fmt::Display for ExpansionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoSuchExpansion => f.write_str("no such expansion"),
            Self::IgnoreExpansion => f.write_str("invalid use of @arg(*)"),
            Self::CommandArgsError(error) => write!(f, "args error: {}", error),
            Self::InvalidArgIndex => f.write_str("invalid arg index"),
            Self::InvalidBufferId => f.write_str("invalid buffer id"),
            Self::NoSuchCommand => f.write_str("no such command"),
            Self::InvalidCursorIndex => f.write_str("invalid cursor index"),
            Self::InvalidRegisterKey => f.write_str("invalid register key"),
            Self::OtherStatic(error) => f.write_str(error),
            Self::OtherOwned(error) => f.write_str(&error),
        }
    }
}
impl From<CommandArgsError> for ExpansionError {
    fn from(other: CommandArgsError) -> Self {
        Self::CommandArgsError(other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Commands,
    Expansions,
    Buffers,
    Files,
    HelpPages,
    Custom(&'static [&'static str]),
}

pub enum CommandArgsError {
    TooFewArguments,
    TooManyArguments,
}
impl fmt::Display for CommandArgsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::TooFewArguments => f.write_str("too few arguments"),
            Self::TooManyArguments => f.write_str("too many arguments"),
        }
    }
}

#[derive(Clone, Copy)]
pub struct CommandArgs<'command>(pub(crate) &'command str);
impl<'command> CommandArgs<'command> {
    pub fn try_next(&mut self) -> Option<&'command str> {
        let i = self.0.find('\0')?;
        let next = &self.0[..i];
        self.0 = &self.0[i + 1..];
        Some(next)
    }

    pub fn next(&mut self) -> Result<&'command str, CommandArgsError> {
        match self.try_next() {
            Some(value) => Ok(value),
            None => Err(CommandArgsError::TooFewArguments),
        }
    }

    pub fn assert_empty(&mut self) -> Result<(), CommandArgsError> {
        match self.try_next() {
            Some(_) => Err(CommandArgsError::TooManyArguments),
            None => Ok(()),
        }
    }
}

pub struct CommandIO<'a> {
    client_handle: Option<ClientHandle>,
    plugin_handle: Option<PluginHandle>,

    pub args: CommandArgs<'a>,
    pub bang: bool,
    pub flow: EditorFlow,
}
impl<'a> CommandIO<'a> {
    pub fn client_handle(&self) -> Result<ClientHandle, CommandError> {
        match self.client_handle {
            Some(handle) => Ok(handle),
            None => Err(CommandError::NoTargetClient),
        }
    }

    pub fn plugin_handle(&self) -> PluginHandle {
        self.plugin_handle.unwrap()
    }

    pub fn current_buffer_view_handle(
        &self,
        ctx: &EditorContext,
    ) -> Result<BufferViewHandle, CommandError> {
        let client_handle = self.client_handle()?;
        match ctx.clients.get(client_handle).buffer_view_handle() {
            Some(handle) => Ok(handle),
            None => Err(CommandError::NoBufferOpened),
        }
    }

    pub fn current_buffer_handle(&self, ctx: &EditorContext) -> Result<BufferHandle, CommandError> {
        let buffer_view_handle = self.current_buffer_view_handle(ctx)?;
        let buffer_handle = ctx
            .editor
            .buffer_views
            .get(buffer_view_handle)
            .buffer_handle;
        Ok(buffer_handle)
    }

    pub fn assert_can_discard_all_buffers(&self, ctx: &EditorContext) -> Result<(), CommandError> {
        if self.bang || !ctx.editor.buffers.iter().any(Buffer::needs_save) {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }

    pub fn assert_can_discard_buffer(
        &self,
        ctx: &EditorContext,
        handle: BufferHandle,
    ) -> Result<(), CommandError> {
        if self.bang || !ctx.editor.buffers.get(handle).needs_save() {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }
}

pub struct CommandIter<'a>(pub &'a str);
impl<'a> Iterator for CommandIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.0 = self.0.trim_start_matches(&[' ', '\t', '\n', '\r']);
            if !self.0.starts_with('#') {
                break;
            }
            match self.0.find('\n') {
                Some(i) => self.0 = &self.0[i + 1..],
                None => self.0 = "",
            }
        }

        let mut tokens = CommandTokenizer(self.0);
        tokens.next()?;
        while tokens.next().is_some() {}
        let len = tokens.0.as_ptr() as usize - self.0.as_ptr() as usize;
        if len == 0 {
            return None;
        }

        let command = &self.0[..len];
        self.0 = tokens.0;

        Some(command)
    }
}

pub struct CommandToken<'a> {
    pub slice: &'a str,
    pub is_simple: bool,
    pub can_expand_variables: bool,
    pub has_escaping: bool,
}

#[derive(Clone)]
pub struct CommandTokenizer<'a>(pub &'a str);
impl<'a> Iterator for CommandTokenizer<'a> {
    type Item = CommandToken<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        fn next_literal_end(s: &str) -> usize {
            match s.find(&[' ', '\t', '\n', '\r', '"', '\'', '{', '}', '#']) {
                Some(0) => 1,
                Some(i) => i,
                None => s.len(),
            }
        }

        fn parse_string_token(delim: char, s: &str) -> Option<(&str, &str, bool)> {
            let mut chars = s.chars();
            let mut has_escaping = false;
            loop {
                match chars.next()? {
                    '\n' | '\r' => break None,
                    '\\' => {
                        chars.next();
                        has_escaping = true;
                    }
                    c if c == delim => {
                        let rest = chars.as_str();
                        let len = rest.as_ptr() as usize - s.as_ptr() as usize - 1;
                        let slice = &s[..len];
                        break Some((slice, rest, has_escaping));
                    }
                    _ => (),
                }
            }
        }

        fn parse_block_token(s: &str) -> Option<(&str, &str)> {
            let mut chars = s.chars();
            let mut balance = 1;
            loop {
                match chars.next()? {
                    '{' => balance += 1,
                    '}' => {
                        balance -= 1;
                        if balance == 0 {
                            let rest = chars.as_str();
                            let len = rest.as_ptr() as usize - s.as_ptr() as usize - 1;
                            break Some((&s[..len], rest));
                        }
                    }
                    '#' => {
                        let rest = chars.as_str();
                        let i = rest.find('\n')?;
                        chars = rest[i + 1..].chars();
                    }
                    '\\' => {
                        chars.next();
                    }
                    _ => (),
                }
            }
        }

        self.0 = self.0.trim_start_matches(&[' ', '\t']);

        let previous_text = self.0;
        let mut can_expand_variables = true;

        loop {
            let mut chars = self.0.chars();
            let next_char = match chars.next() {
                Some(c) => c,
                None => {
                    return if can_expand_variables {
                        None
                    } else {
                        Some(CommandToken {
                            slice: &self.0,
                            is_simple: true,
                            can_expand_variables,
                            has_escaping: false,
                        })
                    }
                }
            };

            match next_char {
                '@' => {
                    can_expand_variables = false;
                    self.0 = chars.as_str();
                }
                delim @ ('"' | '\'') => {
                    let rest = chars.as_str();
                    match parse_string_token(delim, rest) {
                        Some((slice, rest, has_escaping)) => {
                            self.0 = rest;
                            return Some(CommandToken {
                                slice,
                                is_simple: false,
                                can_expand_variables,
                                has_escaping,
                            });
                        }
                        None => {
                            let slice = &self.0[..1];
                            self.0 = rest;
                            return Some(CommandToken {
                                slice,
                                is_simple: false,
                                can_expand_variables,
                                has_escaping: false,
                            });
                        }
                    }
                }
                '\n' | '\r' | '#' => return None,
                c => {
                    if c == '{' {
                        if let Some((slice, rest)) = parse_block_token(chars.as_str()) {
                            self.0 = rest;
                            return Some(CommandToken {
                                slice,
                                is_simple: false,
                                can_expand_variables,
                                has_escaping: false,
                            });
                        }
                    }

                    if !can_expand_variables {
                        can_expand_variables = true;
                        self.0 = previous_text;
                    }

                    let end = next_literal_end(self.0);
                    let (slice, rest) = self.0.split_at(end);
                    self.0 = rest;
                    return Some(CommandToken {
                        slice,
                        is_simple: true,
                        can_expand_variables,
                        has_escaping: false,
                    });
                }
            }
        }
    }
}

pub type CommandFn = fn(ctx: &mut EditorContext, io: &mut CommandIO) -> Result<(), CommandError>;

pub struct Command {
    plugin_handle: Option<PluginHandle>,
    pub completions: &'static [CompletionSource],
    command_fn: CommandFn,
}

struct Macro {
    name_range: Range<u16>,
    source_range: Range<u32>,
}
impl Macro {
    pub fn name<'a>(&self, names: &'a str) -> &'a str {
        &names[self.name_range.start as usize..self.name_range.end as usize]
    }

    pub fn source<'a>(&self, sources: &'a str) -> &'a str {
        &sources[self.source_range.start as usize..self.source_range.end as usize]
    }
}

pub struct ExpansionIO<'a> {
    pub client_handle: Option<ClientHandle>,
    plugin_handle: Option<PluginHandle>,

    pub args: CommandArgs<'a>,
    pub output: &'a mut String,
}
impl<'a> ExpansionIO<'a> {
    pub fn plugin_handle(&self) -> PluginHandle {
        self.plugin_handle.unwrap()
    }

    pub fn current_buffer_view<'ctx>(&self, ctx: &'ctx EditorContext) -> Option<&'ctx BufferView> {
        let client_handle = self.client_handle?;
        let buffer_view_handle = ctx.clients.get(client_handle).buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
        Some(buffer_view)
    }

    pub fn current_buffer<'ctx>(&self, ctx: &'ctx EditorContext) -> Option<&'ctx Buffer> {
        let buffer_view = self.current_buffer_view(ctx)?;
        let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
        Some(buffer)
    }

    pub fn parse_cursor(&self, ctx: &EditorContext, text: &str) -> Result<Option<Cursor>, ExpansionError> {
        let cursors = match self.current_buffer_view(ctx) {
            Some(view) => &view.cursors,
            None => return Ok(None),
        };
        let index = if text.is_empty() {
            cursors.main_cursor_index()
        } else {
            text
                .parse()
                .map_err(|_| ExpansionError::InvalidCursorIndex)?
        };
        Ok(cursors[..].get(index).cloned())
    }
}

pub type ExpansionFn =
    fn(ctx: &mut EditorContext, io: &mut ExpansionIO) -> Result<(), ExpansionError>;

pub struct Expansion {
    plugin_handle: Option<PluginHandle>,
    expansion_fn: ExpansionFn,
}

#[derive(Default)]
pub struct MacroCollection {
    macros: Vec<Macro>,
    names: String,
    sources: String,
}
impl MacroCollection {
    fn add(&mut self, name: &str, source: &str) {
        for (i, m) in self.macros.iter().enumerate() {
            if name == m.name(&self.names) {
                let old_source_range = m.source_range.start as usize..m.source_range.end as usize;
                let old_source_len = old_source_range.end - old_source_range.start;

                let new_source_len = source.len();
                if self.sources.len() - old_source_len + new_source_len > u32::MAX as _ {
                    return;
                }

                self.sources.replace_range(old_source_range, source);

                let old_source_len = old_source_len as u32;
                let new_source_len = new_source_len as u32;

                self.macros[i].source_range.end =
                    self.macros[i].source_range.end - old_source_len + new_source_len;
                for m in &mut self.macros[i + 1..] {
                    m.source_range.start = m.source_range.start - old_source_len + new_source_len;
                    m.source_range.end = m.source_range.end - old_source_len + new_source_len;
                }
                return;
            }
        }

        let name_start = self.names.len();
        let name_end = name_start + name.len();
        if name_end > u16::MAX as _ {
            return;
        }

        let source_start = self.sources.len();
        let source_end = source_start + source.len();
        if source_end > u32::MAX as _ {
            return;
        }

        self.names.push_str(name);
        self.sources.push_str(source);

        self.macros.push(Macro {
            name_range: name_start as _..name_end as _,
            source_range: source_start as _..source_end as _,
        });
    }

    pub fn find(&self, name: &str) -> Option<&str> {
        for m in &self.macros {
            if name == m.name(&self.names) {
                return Some(m.source(&self.sources));
            }
        }
        None
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.macros.iter().map(move |m| m.name(&self.names))
    }
}

struct EvalStackEntry {
    name: String,
    command: String,
    line_index: u32,
}

pub struct CommandManager {
    command_names: Vec<&'static str>,
    commands: Vec<Command>,
    pub macros: MacroCollection,
    expansion_names: Vec<&'static str>,
    expansions: Vec<Expansion>,
    history: VecDeque<String>,
    eval_stack: Vec<EvalStackEntry>,
}

impl CommandManager {
    pub fn new() -> Self {
        let mut this = Self {
            command_names: Vec::new(),
            commands: Vec::new(),
            macros: MacroCollection::default(),
            expansion_names: Vec::new(),
            expansions: Vec::new(),
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
            eval_stack: Vec::new(),
        };

        builtins::register_commands(&mut this);
        expansions::register_expansions(&mut this);

        this
    }

    pub fn register_command(
        &mut self,
        plugin_handle: Option<PluginHandle>,
        name: &'static str,
        completions: &'static [CompletionSource],
        command_fn: CommandFn,
    ) {
        self.command_names.push(name);
        self.commands.push(Command {
            plugin_handle,
            completions,
            command_fn,
        });
    }

    pub fn register_macro(&mut self, name: &str, source: &str) -> Result<(), CommandError> {
        if self.find_command(name).is_some() {
            return Err(CommandError::InvalidMacroName);
        }

        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || matches!(c, '-' | '_') => (),
            _ => return Err(CommandError::InvalidMacroName),
        }
        if name
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_'))
        {
            return Err(CommandError::InvalidMacroName);
        }

        self.macros.add(name, source);
        Ok(())
    }

    pub fn register_expansion(
        &mut self,
        plugin_handle: Option<PluginHandle>,
        name: &'static str,
        expansion_fn: ExpansionFn,
    ) {
        self.expansion_names.push(name);
        self.expansions.push(Expansion {
            plugin_handle,
            expansion_fn,
        });
    }

    pub fn find_command(&self, name: &str) -> Option<&Command> {
        let index = self.command_names.iter().position(|&n| n == name)?;
        Some(&self.commands[index])
    }

    pub fn command_names(&self) -> &[&'static str] {
        &self.command_names
    }

    pub fn expansion_names(&self) -> &[&'static str] {
        &self.expansion_names
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

    pub fn unwrap_eval_result(
        ctx: &mut EditorContext,
        result: Result<EditorFlow, CommandError>,
    ) -> EditorFlow {
        match result {
            Ok(flow) => flow,
            Err(error) => {
                {
                    let mut write = ctx.editor.logger.write(LogKind::Error);
                    write.str("trace:\n");
                    for eval_stack_entry in ctx.editor.commands.eval_stack.drain(..).rev() {
                        write.fmt(format_args!(
                            "\n{}:{}:",
                            eval_stack_entry.name,
                            eval_stack_entry.line_index + 1,
                        ));
                        if eval_stack_entry.command.find('\n').is_some() {
                            for (line_index, line) in eval_stack_entry.command.lines().enumerate() {
                                write.fmt(format_args!("\n    {:>4}| ", line_index + 1));
                                write.str(line);
                            }
                        } else {
                            write.str(" ");
                            write.str(&eval_stack_entry.command);
                        }

                        ctx.editor.string_pool.release(eval_stack_entry.name);
                        ctx.editor.string_pool.release(eval_stack_entry.command);
                    }
                }

                {
                    let mut write = ctx.editor.logger.write(LogKind::Error);
                    write.fmt(format_args!("{}", error));
                }

                EditorFlow::Continue
            }
        }
    }

    pub fn eval(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        name: &str,
        source: &str,
    ) -> Result<EditorFlow, CommandError> {
        Self::eval_recursive(ctx, client_handle, name, source, "", false)
    }

    fn eval_recursive(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        name: &str,
        source: &str,
        args: &str,
        bang: bool,
    ) -> Result<EditorFlow, CommandError> {
        for command in CommandIter(source) {
            let mut aux = ctx.editor.string_pool.acquire();
            let mut expanded = ctx.editor.string_pool.acquire();
            let result = match expand_variables(
                ctx,
                client_handle,
                args,
                bang,
                command,
                &mut aux,
                &mut expanded,
            ) {
                Ok(()) => Self::eval_single(ctx, client_handle, &expanded),
                Err(error) => Err(CommandError::ExpansionError(error)),
            };
            ctx.editor.string_pool.release(aux);
            ctx.editor.string_pool.release(expanded);

            match result {
                Ok(EditorFlow::Continue) => (),
                Ok(flow) => return Ok(flow),
                Err(error) => {
                    let offset = command.as_ptr() as usize - source.as_ptr() as usize;
                    let line_index = source[..offset].chars().filter(|&c| c == '\n').count() as _;

                    ctx.editor.commands.eval_stack.push(EvalStackEntry {
                        name: ctx.editor.string_pool.acquire_with(name),
                        command: ctx.editor.string_pool.acquire_with(command),
                        line_index,
                    });
                    return Err(error);
                }
            }
        }

        Ok(EditorFlow::Continue)
    }

    fn eval_single(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> Result<EditorFlow, CommandError> {
        let mut args = CommandArgs(command);
        let command_name = match args.try_next() {
            Some(command) => command,
            None => return Err(CommandError::NoSuchCommand),
        };
        let (command_name, bang) = match command_name.strip_suffix('!') {
            Some(command) => (command, true),
            None => (command_name, false),
        };

        if let Some(command) = ctx.editor.commands.find_command(command_name) {
            let plugin_handle = command.plugin_handle;
            let command_fn = command.command_fn;
            let mut io = CommandIO {
                client_handle,
                plugin_handle,
                args,
                bang,
                flow: EditorFlow::Continue,
            };
            command_fn(ctx, &mut io)?;
            return Ok(io.flow);
        }

        if let Some(macro_source) = ctx.editor.commands.macros.find(command_name) {
            let macro_source = ctx.editor.string_pool.acquire_with(macro_source);
            let result = Self::eval_recursive(
                ctx,
                client_handle,
                command_name,
                &macro_source,
                args.0,
                bang,
            );
            ctx.editor.string_pool.release(macro_source);
            return result;
        }

        Err(CommandError::NoSuchCommand)
    }
}

fn expand_variables<'a>(
    ctx: &mut EditorContext,
    client_handle: Option<ClientHandle>,
    args: &str,
    bang: bool,
    text: &str,
    aux: &mut String,
    output: &mut String,
) -> Result<(), ExpansionError> {
    fn write_variable_expansion(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        mut command_args: CommandArgs,
        command_bang: bool,
        name: &str,
        args: &str,
        output: &mut String,
    ) -> Result<(), ExpansionError> {
        let mut args = CommandArgs(args);
        if name == "arg" {
            let arg = args.next()?;
            args.assert_empty()?;

            match arg {
                "!" => {
                    if command_bang {
                        output.push('!');
                    }
                }
                "*" => {
                    let command_args = match command_args.0.strip_suffix('\0') {
                        Some(command_args) => command_args,
                        None => return Err(ExpansionError::IgnoreExpansion),
                    };
                    output.push_str(command_args);
                }
                _ => {
                    let mut index: usize =
                        arg.parse().map_err(|_| ExpansionError::InvalidArgIndex)?;
                    while let Some(command_arg) = command_args.try_next() {
                        if index == 0 {
                            output.push_str(command_arg);
                            break;
                        }
                        index -= 1;
                    }
                }
            }
            Ok(())
        } else {
            for (i, &expansion_name) in ctx.editor.commands.expansion_names().iter().enumerate() {
                if expansion_name == name {
                    let expansion = &ctx.editor.commands.expansions[i];
                    let plugin_handle = expansion.plugin_handle;
                    let expansion_fn = expansion.expansion_fn;

                    let mut io = ExpansionIO {
                        client_handle,
                        plugin_handle,
                        args,
                        output,
                    };
                    return expansion_fn(ctx, &mut io);
                }
            }
            Err(ExpansionError::NoSuchExpansion)
        }
    }

    fn parse_variable_name(text: &str) -> Result<&str, usize> {
        let mut chars = text.chars();
        loop {
            match chars.next() {
                Some('a'..='z' | '-') => (),
                Some('(') => {
                    let name = &text[..text.len() - chars.as_str().len() - 1];
                    return Ok(name);
                }
                _ => return Err(text.len() - chars.as_str().len()),
            }
        }
    }

    fn parse_variable_args(text: &str) -> Option<&str> {
        let mut chars = text.chars();
        let mut balance = 1;
        loop {
            match chars.next()? {
                '(' => balance += 1,
                ')' => {
                    balance -= 1;
                    if balance == 0 {
                        let rest = chars.as_str();
                        let len = rest.as_ptr() as usize - text.as_ptr() as usize - 1;
                        break Some(&text[..len]);
                    }
                }
                '\\' => {
                    chars.next();
                }
                _ => (),
            }
        }
    }

    fn write_escaped(mut slice: &str, has_escaping: bool, output: &mut String) {
        if !has_escaping {
            output.push_str(slice);
            return;
        }

        loop {
            match slice.find('\\') {
                Some(i) => {
                    let (before, after) = slice.split_at(i);
                    output.push_str(before);
                    let mut chars = after.chars();
                    chars.next();
                    match chars.next() {
                        Some('t') => output.push('\t'),
                        Some('n') => output.push('\n'),
                        Some(c) => output.push(c),
                        _ => (),
                    }
                    slice = chars.as_str();
                }
                None => {
                    output.push_str(slice);
                    break;
                }
            }
        }
    }

    let aux_prev_len = aux.len();

    'tokens: for token in CommandTokenizer(text) {
        if !token.can_expand_variables {
            write_escaped(token.slice, token.has_escaping, output);
            output.push('\0');
            continue;
        }

        let mut rest = token.slice;
        loop {
            match rest.find('@') {
                Some(i) => {
                    let (before, after) = rest.split_at(i);
                    write_escaped(before, token.has_escaping, output);
                    rest = after;
                }
                None => {
                    write_escaped(rest, token.has_escaping, output);
                    break;
                }
            }

            let variable_name = match parse_variable_name(&rest[1..]) {
                Ok(name) => name,
                Err(skip) => {
                    let (before, after) = rest.split_at(skip + 1);
                    write_escaped(before, token.has_escaping, output);
                    rest = after;
                    continue;
                }
            };

            let args_skip = 1 + variable_name.len() + 1;
            let variable_args = match parse_variable_args(&rest[args_skip..]) {
                Some(args) => args,
                None => {
                    let (before, after) = rest.split_at(args_skip);
                    write_escaped(before, token.has_escaping, output);
                    rest = after;
                    continue;
                }
            };
            rest = &rest[args_skip + variable_args.len() + 1..];

            let aux_len = aux.len();
            expand_variables(ctx, client_handle, args, bang, variable_args, output, aux)?;
            let variable_args = &aux[aux_len..];

            let result = write_variable_expansion(
                ctx,
                client_handle,
                CommandArgs(args),
                bang,
                variable_name,
                variable_args,
                output,
            );
            match result {
                Ok(()) => (),
                Err(ExpansionError::IgnoreExpansion) => {
                    if token.is_simple {
                        continue 'tokens;
                    }
                }
                Err(error) => return Err(error),
            }
        }

        output.push('\0');
    }

    aux.truncate(aux_prev_len);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        env,
        path::{Path, PathBuf},
    };

    use crate::{
        client::ClientManager, editor::Editor, editor_utils::RegisterKey, platform::Platform,
        plugin::PluginCollection,
    };

    #[test]
    fn command_iter() {
        let mut commands = CommandIter("cmd");
        assert_eq!(Some("cmd"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("cmd1\ncmd2");
        assert_eq!(Some("cmd1"), commands.next());
        assert_eq!(Some("cmd2"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("cmd1 {\narg1\n} arg2\ncmd2 {\narg1}\n \t \n ");
        assert_eq!(Some("cmd1 {\narg1\n} arg2"), commands.next());
        assert_eq!(Some("cmd2 {\narg1}"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("cmd1 ' arg\ncmd2 arg'");
        assert_eq!(Some("cmd1 ' arg"), commands.next());
        assert_eq!(Some("cmd2 arg'"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("cmd1 '\ncmd2 arg'");
        assert_eq!(Some("cmd1 '"), commands.next());
        assert_eq!(Some("cmd2 arg'"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter(" #cmd1\ncmd2 arg #arg2\n \t #cmd3 arg\ncmd4 arg'");
        assert_eq!(Some("cmd2 arg "), commands.next());
        assert_eq!(Some("cmd4 arg'"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("cmd1 {\n a\n} b {\n c}\ncmd2");
        assert_eq!(Some("cmd1 {\n a\n} b {\n c}"), commands.next());
        assert_eq!(Some("cmd2"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("cmd1 {\ncmd2 arg #arg2\n \t #cmd3 arg}\ncmd4 arg'}");
        assert_eq!(
            Some("cmd1 {\ncmd2 arg #arg2\n \t #cmd3 arg}\ncmd4 arg'}"),
            commands.next()
        );
        assert_eq!(None, commands.next());
    }

    #[test]
    fn command_tokenizer() {
        let mut tokens = CommandTokenizer("cmd arg1 arg2");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1\\'arg2");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1\\"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1\\'arg2'");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1\\"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd 'arg0 \"arg1 ");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg0"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("\""), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg0'arg1 ");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg0"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg0\"arg1 ");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg0"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("\""), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd \"aaa\\\"bbb\"");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("aaa\\\"bbb"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd 'arg\"0' \"arg'1\"");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg\"0"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg'1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1\"arg2\"arg3");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg3"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1 \" arg2");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("\""), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1 \"arg2");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("\""), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd 'arg1\narg2'");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd 'aaa\\'bbb'");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("aaa\\'bbb"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1 ' arg2");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1 'arg2");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1'arg2'arg3");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg3"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {arg}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {arg\\}}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg\\}"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {aa\\{ bb} arg");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("aa\\{ bb"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg1{arg2}arg3");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg3"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {'}}'}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("}"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("}"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {arg'}{'}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd }arg");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("}"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {arg");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("{"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg}'{\"arg");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("}"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("{"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("\""), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd '{arg}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {\"{(\\\")!\".}|'{(\\')!'.}}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(
            Some("\"{(\\\")!\".}|'{(\\')!'.}"),
            tokens.next().map(|t| t.slice)
        );
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@");
        let token = tokens.next().unwrap();
        assert_eq!("", token.slice);
        assert!(!token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@'aa'");
        let token = tokens.next().unwrap();
        assert_eq!("aa", token.slice);
        assert!(!token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@@'aa'");
        let token = tokens.next().unwrap();
        assert_eq!("aa", token.slice);
        assert!(!token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@{{aa}{}}");
        let token = tokens.next().unwrap();
        assert_eq!("{aa}{}", token.slice);
        assert!(!token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@@{{aa}{}}");
        let token = tokens.next().unwrap();
        assert_eq!("{aa}{}", token.slice);
        assert!(!token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@aa");
        let token = tokens.next().unwrap();
        assert_eq!("@aa", token.slice);
        assert!(token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@@aa");
        let token = tokens.next().unwrap();
        assert_eq!("@@aa", token.slice);
        assert!(token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));
    }

    #[test]
    fn variable_expansion() {
        let current_dir = env::current_dir().unwrap_or(PathBuf::new());
        let mut ctx = EditorContext {
            editor: Editor::new(current_dir, String::new()),
            platform: Platform::default(),
            clients: ClientManager::default(),
            plugins: PluginCollection::default(),
        };

        let register = ctx
            .editor
            .registers
            .get_mut(RegisterKey::from_char('x').unwrap());
        register.clear();
        register.push_str("my register contents");

        let register = ctx
            .editor
            .registers
            .get_mut(RegisterKey::from_char('l').unwrap());
        register.clear();
        register.push_str("very long register contents");

        let register = ctx
            .editor
            .registers
            .get_mut(RegisterKey::from_char('s').unwrap());
        register.clear();
        register.push_str("short");

        let register = ctx
            .editor
            .registers
            .get_mut(RegisterKey::from_char('r').unwrap());
        register.clear();
        register.push_str("x");

        let register = ctx
            .editor
            .registers
            .get_mut(RegisterKey::from_char('t').unwrap());
        register.clear();
        register.push_str("r");

        let buffer = ctx.editor.buffers.add_new();
        assert_eq!(0, buffer.handle().0);
        buffer.set_path(Path::new("buffer/path0"));
        let buffer = ctx.editor.buffers.add_new();
        assert_eq!(1, buffer.handle().0);
        buffer.set_path(Path::new("buffer/veryverylong/path1"));

        let client_handle = ClientHandle(0);
        let buffer_view_handle = ctx
            .editor
            .buffer_views
            .add_new(client_handle, BufferHandle(0));

        ctx.clients.on_client_joined(client_handle);
        ctx.clients
            .get_mut(client_handle)
            .set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);

        fn assert_expansion(expected_expanded: &str, ctx: &mut EditorContext, text: &str) {
            let mut aux = String::new();
            let mut expanded = String::new();
            let result = expand_variables(
                ctx,
                Some(ClientHandle(0)),
                "",
                false,
                text,
                &mut aux,
                &mut expanded,
            );
            if let Err(error) = result {
                panic!("expansion error: {}", error);
            }
            assert_eq!(expected_expanded, &expanded);
        }

        let mut aux = String::new();
        let mut expanded = String::new();

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "  ",
            &mut aux,
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("", &expanded);

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "two args",
            &mut aux,
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("two\0args\0", &expanded);

        assert_expansion("cmd\0", &mut ctx, "cmd");

        assert_expansion("my register contents\0", &mut ctx, "@register(x)");
        assert_expansion(
            "very long register contents short\0",
            &mut ctx,
            "{@register(l) @register(s)}",
        );
        assert_expansion(
            "short very long register contents\0",
            &mut ctx,
            "{@register(s) @register(l)}",
        );

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "@register()",
            &mut aux,
            &mut expanded,
        );
        assert!(matches!(r, Err(ExpansionError::CommandArgsError(CommandArgsError::TooFewArguments))));
        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "@register(xx)",
            &mut aux,
            &mut expanded,
        );
        assert!(matches!(r, Err(ExpansionError::InvalidRegisterKey)));

        assert_expansion("buffer/path0\0", &mut ctx, "@buffer-path()");
        assert_expansion(
            "cmd\0buffer/path0\0asd\0buffer/path0\0",
            &mut ctx,
            "cmd @buffer-path() asd @buffer-path()",
        );

        assert_expansion(
            "cmd\0buffer/path0\0asd\0buffer/veryverylong/path1\0fgh\0\0",
            &mut ctx,
            "cmd @buffer-path(0) asd @buffer-path(1) fgh @buffer-path(2)",
        );

        assert_expansion("\"\0", &mut ctx, "\"\\\"\"");
        assert_expansion("'\0", &mut ctx, "'\\''");
        assert_expansion("\\}\0", &mut ctx, "{\\}}");
        assert_expansion("\\\0", &mut ctx, "'\\\\'");

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(*)",
            &mut aux,
            &mut expanded,
        );
        if let Err(e) = &r {
            eprintln!("aaaaaa ---------------------------- {}", e);
        }
        assert!(r.is_ok());
        assert_eq!("arg0\0arg1\0arg2\0", &expanded);

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "@arg(*)",
            &mut aux,
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("", &expanded);

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(0)",
            &mut aux,
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("arg0\0", &expanded);

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(1)",
            &mut aux,
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("arg1\0", &expanded);

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(2)",
            &mut aux,
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("arg2\0", &expanded);

        expanded.clear();
        let r = expand_variables(
            &mut ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(3)",
            &mut aux,
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("\0", &expanded);

        assert_expansion(
            "my register contents\0",
            &mut ctx,
            "@register(@register(r))",
        );
        assert_expansion(
            "my register contents\0",
            &mut ctx,
            "@register(@register(@register(t)))",
        );
    }
}
