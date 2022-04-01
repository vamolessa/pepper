use std::{collections::VecDeque, env, fmt};

use crate::{
    buffer::{Buffer, BufferHandle, BufferReadError, BufferWriteError},
    buffer_view::{BufferView, BufferViewHandle},
    client::ClientHandle,
    config::ParseConfigError,
    cursor::Cursor,
    editor::{EditorContext, EditorFlow},
    editor_utils::{MessageKind, ParseKeyMapError, RegisterKey},
    events::KeyParseAllError,
    glob::InvalidGlobError,
    pattern::PatternError,
    plugin::PluginHandle,
};

mod builtin;

const HISTORY_CAPACITY: usize = 10;

pub enum CommandError {
    NoSuchCommand,
    TooManyArguments,
    TooFewArguments,
    NoTargetClient,
    NoBufferOpened,
    UnsavedChanges,
    BufferReadError(BufferReadError),
    BufferWriteError(BufferWriteError),
    NoSuchBufferProperty,
    ConfigError(ParseConfigError),
    NoSuchColor,
    InvalidColorValue,
    InvalidModeKind,
    KeyMapError(ParseKeyMapError),
    KeyParseError(KeyParseAllError),
    InvalidRegisterKey,
    InvalidTokenKind,
    PatternError(PatternError),
    InvalidGlob(InvalidGlobError),
    OtherStatic(&'static str),
    OtherOwned(String),
}
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoSuchCommand => f.write_str("no such command"),
            Self::TooManyArguments => f.write_str("too many arguments"),
            Self::TooFewArguments => f.write_str("too few arguments"),
            Self::NoTargetClient => f.write_str("no target client"),
            Self::NoBufferOpened => f.write_str("no buffer opened"),
            Self::UnsavedChanges => f.write_str("unsaved changes"),
            Self::BufferReadError(error) => error.fmt(f),
            Self::BufferWriteError(error) => error.fmt(f),
            Self::NoSuchBufferProperty => f.write_str("no such buffer property"),
            Self::ConfigError(error) => error.fmt(f),
            Self::NoSuchColor => f.write_str("no such color"),
            Self::InvalidColorValue => f.write_str("invalid color value"),
            Self::InvalidModeKind => f.write_str("invalid mode"),
            Self::KeyMapError(error) => error.fmt(f),
            Self::KeyParseError(error) => error.fmt(f),
            Self::InvalidRegisterKey => f.write_str("invalid register key"),
            Self::InvalidTokenKind => f.write_str("invalid token kind"),
            Self::PatternError(error) => error.fmt(f),
            Self::InvalidGlob(error) => error.fmt(f),
            Self::OtherStatic(error) => f.write_str(error),
            Self::OtherOwned(error) => f.write_str(&error),
        }
    }
}

pub struct CommandErrorWithContext {
    pub error: CommandError,
    pub command_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Commands,
    Buffers,
    Files,
    Custom(&'static [&'static str]),
}

pub struct CommandArgs<'command>(&'command str);
impl<'command> CommandArgs<'command> {
    pub fn try_next(&mut self) -> Option<&'command str> {
        let i = self.0.find('\0')?;
        let next = &self.0[..i];
        self.0 = &self.0[i + 1..];
        Some(next)
    }

    pub fn next(&mut self) -> Result<&'command str, CommandError> {
        match self.try_next() {
            Some(value) => Ok(value),
            None => Err(CommandError::TooFewArguments),
        }
    }

    pub fn assert_empty(&mut self) -> Result<(), CommandError> {
        match self.try_next() {
            Some(_) => Err(CommandError::TooManyArguments),
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
            self.0 = self.0.trim_start_matches(&[' ', '\t', '\r', '\n']);
            if !self.0.starts_with('#') {
                break;
            }
            match self.0.find('\n') {
                Some(i) => self.0 = &self.0[i + 1..],
                None => self.0 = "",
            }
        }

        if self.0.is_empty() {
            return None;
        }

        let mut rest = self.0;
        loop {
            rest = rest.trim_start_matches(&[' ', '\t', '\r']);
            if rest.starts_with('\n') {
                let len = rest.as_ptr() as usize - self.0.as_ptr() as usize;
                let command = &self.0[..len];
                self.0 = rest;
                return Some(command);
            }

            let mut tokens = CommandTokenizer(rest);
            match tokens.next() {
                Some(_) => rest = tokens.0,
                None => {
                    let command = self.0;
                    self.0 = "";
                    return Some(command);
                }
            }
        }
    }
}

pub struct CommandToken<'a> {
    pub slice: &'a str,
    pub can_expand_variables: bool,
    pub has_escaping: bool,
}

#[derive(Clone)]
pub struct CommandTokenizer<'a>(pub &'a str);
impl<'a> Iterator for CommandTokenizer<'a> {
    type Item = CommandToken<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        fn next_literal_end(s: &str) -> usize {
            match s.find(&[' ', '\t', '\r', '\n', '"', '\'', '{', '}']) {
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

        fn parse_block_token(s: &str) -> Option<(&str, &str, bool)> {
            let mut chars = s.chars();
            let mut balance = 1;
            let mut has_escaping = false;
            loop {
                match chars.next()? {
                    '{' => balance += 1,
                    '}' => {
                        balance -= 1;
                        if balance == 0 {
                            let rest = chars.as_str();
                            let len = rest.as_ptr() as usize - s.as_ptr() as usize - 1;
                            break Some((&s[..len], rest, has_escaping));
                        }
                    }
                    delim @ ('"' | '\'') => {
                        let rest = chars.as_str();
                        let rest = match parse_string_token(delim, rest) {
                            Some((_, rest, _)) => rest,
                            None => rest,
                        };
                        chars = rest.chars();
                    }
                    '\n' => {
                        let mut rest = chars.as_str().trim_start_matches(&[' ', '\t']);
                        if rest.starts_with('#') {
                            let i = rest.find('\n')?;
                            rest = &rest[i..];
                        }
                        chars = rest.chars();
                    }
                    '\\' => {
                        chars.next();
                        has_escaping = true;
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
            match chars.next()? {
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
                                can_expand_variables,
                                has_escaping,
                            });
                        }
                        None => {
                            let slice = &self.0[..1];
                            self.0 = rest;
                            return Some(CommandToken {
                                slice,
                                can_expand_variables,
                                has_escaping: false,
                            });
                        }
                    }
                }
                '\n' | '\r' => return None,
                c => {
                    if c == '{' {
                        if let Some((slice, rest, has_escaping)) = parse_block_token(chars.as_str())
                        {
                            self.0 = rest;
                            return Some(CommandToken {
                                slice,
                                can_expand_variables,
                                has_escaping,
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
    pub name: &'static str,
    pub completions: &'static [CompletionSource],
    command_fn: CommandFn,
}

struct Macro {
    name_start: u16,
    name_end: u16,
    source_start: u32,
    source_end: u32,
}
impl Macro {
    pub fn name<'a>(&self, names: &'a str) -> &'a str {
        &names[self.name_start as usize..self.name_end as usize]
    }

    pub fn source<'a>(&self, sources: &'a str) -> &'a str {
        &sources[self.source_start as usize..self.source_end as usize]
    }
}

#[derive(Default)]
pub struct MacroCollection {
    macros: Vec<Macro>,
    names: String,
    sources: String,
}
impl MacroCollection {
    pub fn add(&mut self, name: &str, source: &str) {
        for (i, m) in self.macros.iter().enumerate() {
            if name == m.name(&self.names) {
                let old_source_range = m.source_start as usize..m.source_end as usize;
                let old_source_len = old_source_range.end - old_source_range.start;

                let new_source_len = source.len();
                if self.sources.len() - old_source_len + new_source_len > u32::MAX as _ {
                    return;
                }

                self.sources.replace_range(old_source_range, source);

                let old_source_len = old_source_len as u32;
                let new_source_len = new_source_len as u32;

                self.macros[i].source_end = self.macros[i].source_end - old_source_len + new_source_len;
                for m in &mut self.macros[i + 1..] {
                    m.source_start = m.source_start - old_source_len + new_source_len;
                    m.source_end = m.source_end - old_source_len + new_source_len;
                }
                return;
            }
        }

        let name_start = self.names.len();
        let name_end = name_start + name.len();
        if name_end > u32::MAX as _ {
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
            name_start: name_start as _,
            name_end: name_end as _,
            source_start: source_start as _,
            source_end: source_end as _,
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

struct Alias {
    start: u32,
    from_len: u16,
    to_len: u16,
}
impl Alias {
    pub fn from<'a>(&self, texts: &'a str) -> &'a str {
        let end = self.start as usize + self.from_len as usize;
        &texts[self.start as usize..end]
    }

    pub fn to<'a>(&self, texts: &'a str) -> &'a str {
        let start = self.start as usize + self.from_len as usize;
        let end = start + self.to_len as usize;
        &texts[start..end]
    }
}

#[derive(Default)]
pub struct AliasCollection {
    texts: String,
    aliases: Vec<Alias>,
}
impl AliasCollection {
    pub fn add(&mut self, from: &str, to: &str) {
        if from.len() > u16::MAX as _ || to.len() > u16::MAX as _ {
            return;
        }

        for (i, alias) in self.aliases.iter().enumerate() {
            if from == alias.from(&self.texts) {
                let alias_start = alias.start as usize;
                let alias_len = alias.from_len as u32 + alias.to_len as u32;
                self.aliases.remove(i);
                for alias in &mut self.aliases[i..] {
                    alias.start -= alias_len;
                }
                self.texts
                    .drain(alias_start..alias_start + alias_len as usize);
                break;
            }
        }

        let start = self.texts.len() as _;
        self.texts.push_str(from);
        self.texts.push_str(to);

        self.aliases.push(Alias {
            start,
            from_len: from.len() as _,
            to_len: to.len() as _,
        });
    }

    pub fn find(&self, from: &str) -> Option<&str> {
        for alias in &self.aliases {
            if from == alias.from(&self.texts) {
                return Some(alias.to(&self.texts));
            }
        }

        None
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.aliases.iter().map(move |a| a.from(&self.texts))
    }
}

pub struct CommandManager {
    commands: Vec<Command>,
    pub macros: MacroCollection,
    history: VecDeque<String>,
    pub aliases: AliasCollection,
}

impl CommandManager {
    pub fn new() -> Self {
        let mut this = Self {
            commands: Vec::new(),
            macros: MacroCollection::default(),
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
            aliases: AliasCollection::default(),
        };
        builtin::register_commands(&mut this);
        this
    }

    pub fn register_command(
        &mut self,
        plugin_handle: Option<PluginHandle>,
        name: &'static str,
        completions: &'static [CompletionSource],
        command_fn: CommandFn,
    ) {
        self.commands.push(Command {
            plugin_handle,
            name,
            completions,
            command_fn,
        });
    }

    pub fn find_command(&self, name: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.name == name)
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
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
        result: Result<EditorFlow, CommandErrorWithContext>,
        source: &str,
        name: Option<&str>,
    ) -> EditorFlow {
        match result {
            Ok(flow) => flow,
            Err(error) => {
                let command = match CommandIter(source).nth(error.command_index) {
                    Some(command) => command,
                    None => &source[..0],
                };
                let offset = command.as_ptr() as usize - source.as_ptr() as usize;
                let line_index = source[..offset].chars().filter(|&c| c == '\n').count();

                let mut write = ctx.editor.status_bar.write(MessageKind::Error);
                match name {
                    Some(name) => write.fmt(format_args!(
                        "{}:{}\n{}\n{}",
                        name,
                        line_index + 1,
                        command,
                        error.error,
                    )),
                    None => write.fmt(format_args!("{}", error.error)),
                }

                EditorFlow::Continue
            }
        }
    }

    pub fn eval(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        source: &str,
    ) -> Result<EditorFlow, CommandErrorWithContext> {
        for (i, command) in CommandIter(source).enumerate() {
            match Self::eval_single(ctx, client_handle, command) {
                Ok(EditorFlow::Continue) => (),
                Ok(flow) => return Ok(flow),
                Err(error) => {
                    return Err(CommandErrorWithContext {
                        error,
                        command_index: i,
                    });
                }
            }
        }

        Ok(EditorFlow::Continue)
    }

    pub fn eval_single(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        mut command: &str,
    ) -> Result<EditorFlow, CommandError> {
        let mut expanded = ctx.editor.string_pool.acquire();

        let mut force_bang = false;
        let mut tokens = CommandTokenizer(command);
        if let Some(token) = tokens.next() {
            let alias = match token.slice.strip_suffix('!') {
                Some(token) => {
                    force_bang = true;
                    token
                }
                None => token.slice,
            };
            if let Some(aliased) = ctx.editor.commands.aliases.find(alias) {
                expand_variables(ctx, client_handle, aliased, &mut expanded);
                command = tokens.0;
            }
        }

        expand_variables(ctx, client_handle, command, &mut expanded);

        let result = Self::eval_single_impl(ctx, client_handle, &expanded, force_bang);
        ctx.editor.string_pool.release(expanded);
        result
    }

    fn eval_single_impl(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        command: &str,
        force_bang: bool,
    ) -> Result<EditorFlow, CommandError> {
        let mut args = CommandArgs(command);
        let command_name = match args.try_next() {
            Some(command) => command,
            None => return Err(CommandError::NoSuchCommand),
        };
        let (command_name, bang) = match command_name.strip_suffix('!') {
            Some(command) => (command, true),
            None => (command_name, force_bang),
        };

        if let Some(macro_source) = ctx.editor.commands.macros.find(command_name) {
            let macro_source = ctx.editor.string_pool.acquire_with(macro_source);
            let mut result = Ok(EditorFlow::Continue);
            for command in CommandIter(&macro_source) {
                result = Self::eval_single(ctx, client_handle, command);
            }
            ctx.editor.string_pool.release(macro_source);
            return result;
        }

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

        Err(CommandError::NoSuchCommand)
    }
}

fn write_variable_expansion<'ctx>(
    ctx: &'ctx EditorContext,
    client_handle: Option<ClientHandle>,
    name: &str,
    args: &str,
    output: &mut String,
) -> Option<()> {
    fn assert_empty_args(args: &str) -> Option<()> {
        if args.is_empty() {
            Some(())
        } else {
            None
        }
    }

    fn current_buffer_view(
        ctx: &EditorContext,
        client_handle: Option<ClientHandle>,
    ) -> Option<&BufferView> {
        let buffer_view_handle = ctx.clients.get(client_handle?).buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
        Some(buffer_view)
    }

    fn current_buffer(ctx: &EditorContext, client_handle: Option<ClientHandle>) -> Option<&Buffer> {
        let buffer_view = current_buffer_view(ctx, client_handle)?;
        let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
        Some(buffer)
    }

    fn cursor(
        ctx: &EditorContext,
        client_handle: Option<ClientHandle>,
        args: &str,
    ) -> Option<Cursor> {
        let cursors = &current_buffer_view(ctx, client_handle)?.cursors;
        let index = if args.is_empty() {
            cursors.main_cursor_index()
        } else {
            args.parse().ok()?
        };
        cursors[..].get(index).cloned()
    }

    use fmt::Write;

    match name {
        "client-id" => {
            assert_empty_args(args)?;
            let _ = write!(output, "{}", client_handle?.0);
        }
        "buffer-id" => {
            assert_empty_args(args)?;
            let buffer = current_buffer(ctx, client_handle)?;
            let _ = write!(output, "{}", buffer.handle().0);
        }
        "buffer-path" => {
            let buffer = if args.is_empty() {
                current_buffer(ctx, client_handle)?
            } else {
                let handle = BufferHandle(args.parse().ok()?);
                ctx.editor.buffers.try_get(handle)?
            };
            output.push_str(buffer.path.to_str()?);
        }
        "buffer-content" => {
            let buffer = if args.is_empty() {
                current_buffer(ctx, client_handle)?
            } else {
                let handle = BufferHandle(args.parse().ok()?);
                ctx.editor.buffers.try_get(handle)?
            };
            for line in buffer.content().lines() {
                output.push_str(line.as_str());
                output.push('\n');
            }
        }
        "cursor-anchor-column" => {
            let cursor = cursor(ctx, client_handle, args)?;
            let _ = write!(output, "{}", cursor.anchor.column_byte_index);
        }
        "cursor-anchor-line" => {
            let cursor = cursor(ctx, client_handle, args)?;
            let _ = write!(output, "{}", cursor.anchor.line_index);
        }
        "cursor-position-column" => {
            let cursor = cursor(ctx, client_handle, args)?;
            let _ = write!(output, "{}", cursor.position.column_byte_index);
        }
        "cursor-position-line" => {
            let cursor = cursor(ctx, client_handle, args)?;
            let _ = write!(output, "{}", cursor.position.line_index);
        }
        "cursor-selection" => {
            let buffer = current_buffer(ctx, client_handle)?;
            let range = cursor(ctx, client_handle, args)?.to_range();
            for text in buffer.content().text_range(range) {
                output.push_str(text);
            }
        }
        "readline-input" => {
            assert_empty_args(args)?;
            output.push_str(ctx.editor.read_line.input());
        }
        "picker-entry" => {
            assert_empty_args(args)?;
            let entry = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
                Some(entry) => entry.1,
                None => "",
            };
            output.push_str(entry);
        }
        "register" => {
            let key = RegisterKey::from_str(args)?;
            output.push_str(ctx.editor.registers.get(key));
        }
        "env" => {
            let env_var = env::var(args).unwrap_or(String::new());
            output.push_str(&env_var);
        }
        "pid" => {
            assert_empty_args(args)?;
            let _ = write!(output, "{}", std::process::id());
        }
        _ => (),
    }

    Some(())
}

fn expand_variables<'a>(
    ctx: &EditorContext,
    client_handle: Option<ClientHandle>,
    text: &str,
    output: &mut String,
) {
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
        let i = text.find(')')?;
        Some(&text[..i])
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

    for token in CommandTokenizer(text) {
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

            if write_variable_expansion(ctx, client_handle, variable_name, variable_args, output)
                .is_none()
            {
                output.push('@');
                output.push_str(variable_name);
                output.push('(');
                output.push_str(variable_args);
                output.push(')');
            }
        }

        output.push('\0');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        env,
        path::{Path, PathBuf},
    };

    use crate::{
        client::ClientManager, editor::Editor, platform::Platform, plugin::PluginCollection,
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
        assert_eq!(Some("cmd2 arg #arg2"), commands.next());
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
        assert_eq!(Some("'}}'"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {arg'}}=}}=='}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg'}}=}}=='"), tokens.next().map(|t| t.slice));
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

        let mut tokens = CommandTokenizer("cmd {arg'}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg'"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {\"{(\\\")!\".\\}|'{(\\')!'.\\}}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(
            Some("\"{(\\\")!\".\\}|'{(\\')!'.\\}"),
            tokens.next().map(|t| t.slice)
        );
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
            editor: Editor::new(current_dir),
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

        fn assert_expansion(expected_expanded: &str, ctx: &EditorContext, text: &str) {
            let mut expanded = String::new();
            expand_variables(ctx, Some(ClientHandle(0)), text, &mut expanded);
            assert_eq!(expected_expanded, &expanded);
        }

        let mut expanded = String::new();
        expand_variables(&ctx, Some(ClientHandle(0)), "  ", &mut expanded);
        assert_eq!("", &expanded);

        let mut expanded = String::new();
        expand_variables(&ctx, Some(ClientHandle(0)), "two args", &mut expanded);
        assert_eq!("two\0args\0", &expanded);

        assert_expansion("cmd\0", &ctx, "cmd");

        assert_expansion("my register contents\0", &ctx, "@register(x)");
        assert_expansion("@register()\0", &ctx, "@register()");
        assert_expansion("@register(xx)\0", &ctx, "@register(xx)");
        assert_expansion(
            "very long register contents short\0",
            &ctx,
            "{@register(l) @register(s)}",
        );
        assert_expansion(
            "short very long register contents\0",
            &ctx,
            "{@register(s) @register(l)}",
        );

        assert_expansion("buffer/path0\0", &ctx, "@buffer-path()");
        assert_expansion(
            "cmd\0buffer/path0\0asd\0buffer/path0\0",
            &ctx,
            "cmd @buffer-path() asd @buffer-path()",
        );

        assert_expansion(
            "cmd\0buffer/path0\0asd\0buffer/veryverylong/path1\0fgh\0@buffer-path(2)\0",
            &ctx,
            "cmd @buffer-path(0) asd @buffer-path(1) fgh @buffer-path(2)",
        );

        assert_expansion("\"\0", &ctx, "\"\\\"\"");
        assert_expansion("'\0", &ctx, "'\\''");
        assert_expansion("}\0", &ctx, "{\\}}");
        assert_expansion("\\\0", &ctx, "'\\\\'");
    }
}

