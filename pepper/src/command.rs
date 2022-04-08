use std::{collections::VecDeque, fmt};

use crate::{
    buffer::{Buffer, BufferHandle, BufferReadError, BufferWriteError},
    buffer_view::BufferViewHandle,
    client::ClientHandle,
    config::ParseConfigError,
    editor::{EditorContext, EditorFlow},
    editor_utils::{MessageKind, ParseKeyMapError},
    events::KeyParseAllError,
    glob::InvalidGlobError,
    pattern::PatternError,
    plugin::PluginHandle,
};

mod builtins;
mod expansions;

const HISTORY_CAPACITY: usize = 10;

pub enum CommandError {
    InvalidMacroName,
    ExpansionError(expansions::ExpansionError),
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
            Self::InvalidMacroName => f.write_str("invalid command name"),
            Self::ExpansionError(error) => write!(f, "expansion error: {}", error),
            Self::NoSuchCommand => f.write_str("no such command"),
            Self::TooManyArguments => f.write_str("too many arguments"),
            Self::TooFewArguments => f.write_str("too few arguments"),
            Self::NoTargetClient => f.write_str("no target client"),
            Self::NoBufferOpened => f.write_str("no buffer opened"),
            Self::UnsavedChanges => f.write_str("unsaved changes"),
            Self::BufferReadError(error) => write!(f, "buffer read error: {}", error),
            Self::BufferWriteError(error) => write!(f, "buffer write error: {}", error),
            Self::NoSuchBufferProperty => f.write_str("no such buffer property"),
            Self::ConfigError(error) => write!(f, "config error: {}", error),
            Self::NoSuchColor => f.write_str("no such color"),
            Self::InvalidColorValue => f.write_str("invalid color value"),
            Self::InvalidModeKind => f.write_str("invalid mode"),
            Self::KeyMapError(error) => write!(f, "key map error: {}", error),
            Self::KeyParseError(error) => write!(f, "key parse error: {}", error),
            Self::InvalidRegisterKey => f.write_str("invalid register key"),
            Self::InvalidTokenKind => f.write_str("invalid token kind"),
            Self::PatternError(error) => write!(f, "pattern error: {}", error),
            Self::InvalidGlob(error) => write!(f, "glob error: {}", error),
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

pub struct CommandArgs<'command>(pub(crate) &'command str);
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
                    '#' => {
                        let rest = chars.as_str();
                        let i = rest.find('\n')?;
                        chars = rest[i + 1..].chars();
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
                '\n' | '\r' | '#' => return None,
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
    fn add(&mut self, name: &str, source: &str) {
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

                self.macros[i].source_end =
                    self.macros[i].source_end - old_source_len + new_source_len;
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

pub struct CommandManager {
    commands: Vec<Command>,
    pub macros: MacroCollection,
    history: VecDeque<String>,
}

impl CommandManager {
    pub fn new() -> Self {
        let mut this = Self {
            commands: Vec::new(),
            macros: MacroCollection::default(),
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
        };
        builtins::register_commands(&mut this);
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

    pub fn register_macro(&mut self, name: &str, source: &str) -> Result<(), CommandError> {
        if self.find_command(name).is_some() {
            return Err(CommandError::InvalidMacroName);
        }

        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || matches!(c, '-' | '_') => (),
            _ => return Err(CommandError::InvalidMacroName),
        }
        if name.chars().any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_')) {
            return Err(CommandError::InvalidMacroName);
        }

        self.macros.add(name, source);
        Ok(())
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
            match Self::eval_single(ctx, client_handle, command, "", false) {
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

    pub(crate) fn eval_single(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        command: &str,
        args: &str,
        bang: bool,
    ) -> Result<EditorFlow, CommandError> {
        let mut expanded = ctx.editor.string_pool.acquire();
        let result = match expand_variables(ctx, client_handle, args, bang, command, &mut expanded)
        {
            Ok(()) => Self::eval_single_impl(ctx, client_handle, &expanded),
            Err(error) => Err(CommandError::ExpansionError(error)),
        };
        ctx.editor.string_pool.release(expanded);
        result
    }

    fn eval_single_impl(
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
            let mut result = Ok(EditorFlow::Continue);
            for command in CommandIter(&macro_source) {
                result = Self::eval_single(ctx, client_handle, command, args.0, bang);
            }
            ctx.editor.string_pool.release(macro_source);
            return result;
        }

        Err(CommandError::NoSuchCommand)
    }
}

fn expand_variables<'a>(
    ctx: &EditorContext,
    client_handle: Option<ClientHandle>,
    args: &str,
    bang: bool,
    text: &str,
    output: &mut String,
) -> Result<(), expansions::ExpansionError> {
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

            expansions::write_variable_expansion(
                ctx,
                client_handle,
                CommandArgs(args),
                bang,
                variable_name,
                variable_args,
                output,
            )?;
        }

        output.push('\0');
    }

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
            let result =
                expand_variables(ctx, Some(ClientHandle(0)), "", false, text, &mut expanded);
            if let Err(error) = result {
                panic!("expansion error: {}", error);
            }
            assert_eq!(expected_expanded, &expanded);
        }

        let mut expanded = String::new();
        let r = expand_variables(&ctx, Some(ClientHandle(0)), "", false, "  ", &mut expanded);
        assert!(r.is_ok());
        assert_eq!("", &expanded);

        let mut expanded = String::new();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "two args",
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("two\0args\0", &expanded);

        assert_expansion("cmd\0", &ctx, "cmd");

        assert_expansion("my register contents\0", &ctx, "@register(x)");
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

        let mut expanded = String::new();
        expanded.clear();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "@register()",
            &mut expanded,
        );
        assert!(matches!(
            r,
            Err(expansions::ExpansionError::InvalidRegisterKey)
        ));
        expanded.clear();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "",
            false,
            "@register(xx)",
            &mut expanded,
        );
        assert!(matches!(
            r,
            Err(expansions::ExpansionError::InvalidRegisterKey)
        ));

        assert_expansion("buffer/path0\0", &ctx, "@buffer-path()");
        assert_expansion(
            "cmd\0buffer/path0\0asd\0buffer/path0\0",
            &ctx,
            "cmd @buffer-path() asd @buffer-path()",
        );

        assert_expansion(
            "cmd\0buffer/path0\0asd\0buffer/veryverylong/path1\0fgh\0\0",
            &ctx,
            "cmd @buffer-path(0) asd @buffer-path(1) fgh @buffer-path(2)",
        );

        assert_expansion("\"\0", &ctx, "\"\\\"\"");
        assert_expansion("'\0", &ctx, "'\\''");
        assert_expansion("}\0", &ctx, "{\\}}");
        assert_expansion("\\\0", &ctx, "'\\\\'");

        let mut expanded = String::new();
        expanded.clear();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(*)",
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("arg0\0arg1\0arg2\0", &expanded);
        expanded.clear();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(0)",
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("arg0\0", &expanded);
        expanded.clear();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(1)",
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("arg1\0", &expanded);
        expanded.clear();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(2)",
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("arg2\0", &expanded);
        expanded.clear();
        let r = expand_variables(
            &ctx,
            Some(ClientHandle(0)),
            "arg0\0arg1\0arg2\0",
            false,
            "@arg(3)",
            &mut expanded,
        );
        assert!(r.is_ok());
        assert_eq!("\0", &expanded);
    }
}
