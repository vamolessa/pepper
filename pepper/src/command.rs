use std::{collections::VecDeque, fmt};

use crate::{
    buffer::{Buffer, BufferHandle, BufferReadError, BufferWriteError},
    cursor::Cursor,
    buffer_view::{BufferViewHandle, BufferView},
    client::ClientHandle,
    config::ParseConfigError,
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
    KeyMapError(ParseKeyMapError),
    KeyParseError(KeyParseAllError),
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
            Self::KeyMapError(error) => error.fmt(f),
            Self::KeyParseError(error) => error.fmt(f),
            Self::PatternError(error) => error.fmt(f),
            Self::InvalidGlob(error) => error.fmt(f),
            Self::OtherStatic(error) => f.write_str(error),
            Self::OtherOwned(error) => f.write_str(&error),
        }
    }
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

const WHITESPACE: &[char] = &[' ', '\t', '\r', '\n'];

pub struct CommandIter<'a>(pub &'a str);
impl<'a> Iterator for CommandIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.0 = self.0.trim_start_matches(WHITESPACE);
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
    pub can_expand_variables: bool,
    pub slice: &'a str,
}

#[derive(Clone)]
pub struct CommandTokenizer<'a>(pub &'a str);
impl<'a> Iterator for CommandTokenizer<'a> {
    type Item = CommandToken<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        fn next_literal_end(s: &str) -> usize {
            match s.find(WHITESPACE) {
                Some(i) => i,
                None => s.len(),
            }
        }

        fn parse_balanced_command_token(s: &str) -> Option<(&str, &str)> {
            let mut chars = s.chars();
            let mut depth = 0;
            loop {
                match chars.next()? {
                    '=' => depth += 1,
                    '{' => break,
                    _ => return None,
                }
            }
            let start = chars.as_str().as_ptr() as usize;
            let mut end = start;
            let mut ending = false;
            let mut matched = 0;
            loop {
                match chars.next()? {
                    '}' => {
                        if ending && matched == depth {
                            break;
                        }

                        ending = true;
                        matched = 0;
                        end = chars.as_str().as_ptr() as usize - 1;
                    }
                    '=' => matched += 1,
                    _ => ending = false,
                }
            }
            let rest = chars.as_str();
            let base = s.as_ptr() as usize;
            let start = start - base;
            let end = end - base;
            let token = &s[start..end];

            Some((token, rest))
        }
        self.0 = self.0.trim_start_matches(WHITESPACE);

        let previous_text = self.0;
        let mut can_expand_variables = true;
        loop {
            match self.0.chars().next()? {
                '@' => {
                    can_expand_variables = false;
                    self.0 = &self.0[1..];
                }
                delim @ ('"' | '\'') => {
                    let rest = &self.0[1..];
                    match rest.find(&[delim, '\n']) {
                        Some(i) if rest[i..].starts_with(delim) => {
                            let slice = &rest[..i];
                            self.0 = &rest[i + 1..];
                            return Some(CommandToken {
                                can_expand_variables,
                                slice,
                            });
                        }
                        _ => {
                            let end = next_literal_end(rest);
                            let (slice, rest) = self.0.split_at(end + 1);
                            self.0 = rest;
                            return Some(CommandToken {
                                can_expand_variables,
                                slice,
                            });
                        }
                    }
                }
                c => {
                    if c == '{' {
                        if let Some((slice, rest)) = parse_balanced_command_token(&self.0[1..]) {
                            self.0 = rest;
                            return Some(CommandToken {
                                can_expand_variables,
                                slice,
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
                        can_expand_variables,
                        slice,
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

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.aliases
            .iter()
            .map(move |a| (a.from(&self.texts), a.to(&self.texts)))
    }
}

pub struct CommandManager {
    commands: Vec<Command>,
    history: VecDeque<String>,
    pub aliases: AliasCollection,
}

impl CommandManager {
    pub fn new() -> Self {
        let mut this = Self {
            commands: Vec::new(),
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

    pub fn eval_and_write_error(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> EditorFlow {
        match Self::try_eval(ctx, client_handle, command) {
            Ok(flow) => flow,
            Err(error) => {
                ctx.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error));
                EditorFlow::Continue
            }
        }
    }

    pub fn try_eval(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        mut command: &str,
    ) -> Result<EditorFlow, CommandError> {
        let mut expanded = ctx.editor.string_pool.acquire();

        let mut tokens = CommandTokenizer(command);
        if let Some(token) = tokens.next() {
            let alias = token.slice.trim_end_matches('!');
            if let Some(aliased) = ctx.editor.commands.aliases.find(alias) {
                expand_variables(ctx, client_handle, aliased, &mut expanded);
                command = tokens.0;
            }
        }

        expand_variables(ctx, client_handle, command, &mut expanded);

        let result = Self::eval(ctx, client_handle, &expanded);
        ctx.editor.string_pool.release(expanded);
        result
    }

    fn eval(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> Result<EditorFlow, CommandError> {
        let mut args = CommandArgs(command);
        let command = match args.try_next() {
            Some(command) => command,
            None => return Err(CommandError::NoSuchCommand),
        };
        let (command, bang) = match command.strip_suffix('!') {
            Some(command) => (command, true),
            None => (command, false),
        };
        let (plugin_handle, command_fn) = match ctx.editor.commands.find_command(command) {
            Some(command) => (command.plugin_handle, command.command_fn),
            None => return Err(CommandError::NoSuchCommand),
        };

        let mut io = CommandIO {
            client_handle,
            plugin_handle,
            args,
            bang,
            flow: EditorFlow::Continue,
        };
        command_fn(ctx, &mut io)?;
        Ok(io.flow)
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

    fn current_buffer_view(ctx: &EditorContext, client_handle: Option<ClientHandle>) -> Option<&BufferView> {
        let buffer_view_handle = ctx.clients.get(client_handle?).buffer_view_handle()?;
        let buffer_view = ctx
            .editor
            .buffer_views
            .get(buffer_view_handle);
        Some(buffer_view)
    }

    fn current_buffer(ctx: &EditorContext, client_handle: Option<ClientHandle>) -> Option<&Buffer> {
        let buffer_view = current_buffer_view(ctx, client_handle)?;
        let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
        Some(buffer)
    }

    fn cursor(ctx: &EditorContext, client_handle: Option<ClientHandle>, args: &str) -> Option<Cursor> {
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
        "cursor-count" => {
            assert_empty_args(args)?;
            let buffer_view = current_buffer_view(ctx, client_handle)?;
            let _ = write!(output, "{}", buffer_view.cursors[..].len());
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
        "readline-input" => {
            assert_empty_args(args)?;
            output.push_str(ctx.editor.read_line.input());
        }
        "register" => {
            let key = RegisterKey::from_str(args)?;
            output.push_str(ctx.editor.registers.get(key));
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

    for token in CommandTokenizer(text) {
        if !token.can_expand_variables {
            output.push_str(token.slice);
            output.push('\0');
            continue;
        }

        let mut rest = token.slice;
        loop {
            match rest.find('@') {
                Some(i) => {
                    let (before, after) = rest.split_at(i);
                    output.push_str(before);
                    rest = after;
                }
                None => {
                    output.push_str(rest);
                    break;
                }
            }

            let variable_name = match parse_variable_name(&rest[1..]) {
                Ok(name) => name,
                Err(skip) => {
                    let (before, after) = rest.split_at(skip + 1);
                    output.push_str(before);
                    rest = after;
                    continue;
                }
            };

            let args_skip = 1 + variable_name.len() + 1;
            let variable_args = match parse_variable_args(&rest[args_skip..]) {
                Some(args) => args,
                None => {
                    let (before, after) = rest.split_at(args_skip);
                    output.push_str(before);
                    rest = after;
                    continue;
                }
            };

            rest = &rest[args_skip + variable_args.len() + 1..];

            if write_variable_expansion(ctx, client_handle, variable_name, variable_args, output).is_none() {
                output.push('@');
                output.push_str(variable_name);
                output.push('(');
                output.push_str(variable_args);
                output.push(')');
            }
        }

        output.push('\0');
    }

    /*
    let mut rest_index = 0;
    let mut token_count = 0;

    loop {
        let text_ptr = text.as_ptr() as usize;
        let mut tokens = CommandTokenizer(&text[rest_index..]);
        let token = match tokens.next() {
            Some(token) => token,
            None => return,
        };
        rest_index = tokens.0.as_ptr() as usize - text_ptr;

        let token_start = token.slice.as_ptr() as usize - text_ptr;
        let mut token_rest_index = token_start;
        let mut token_end = token_rest_index + token.slice.len();

        if !token.can_expand_variables {
            continue;
        }

        loop {
            let token = &text[token_rest_index..token_end];
            let variable_start = match token.find('@') {
                Some(i) => token_rest_index + i,
                None => break,
            };
            let variable_name = match parse_variable_name(&text[variable_start + 1..]) {
                Ok(name) => name,
                Err(skip) => {
                    token_rest_index += skip;
                    continue;
                }
            };
            token_rest_index = variable_start + 1 + variable_name.len() + 1;
            let variable_args = match parse_variable_args(&text[token_rest_index..]) {
                Some(args) => args,
                None => continue,
            };
            token_rest_index += variable_args.len() + 1;

            let variable_value = match get_expansion_variable_value(
                ctx,
                client_handle,
                variable_name,
                variable_args,
                &mut write_int_buf,
            ) {
                Some(value) => value,
                None => continue,
            };

            text.replace_range(variable_start..token_rest_index, variable_value);

            let variable_len = token_rest_index - variable_start;
            if variable_value.len() < variable_len {
                let delta = variable_len - variable_value.len();
                rest_index -= delta;
                token_rest_index -= delta;
                token_end -= delta;
            } else {
                let delta = variable_value.len() - variable_len;
                rest_index += delta;
                token_rest_index += delta;
                token_end += delta;
            }
        }
    }
    */
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

        let mut commands = CommandIter("cmd1 {{\narg1\n}} arg2\ncmd2 {={\narg1}=}\n \t \n ");
        assert_eq!(Some("cmd1 {{\narg1\n}} arg2"), commands.next());
        assert_eq!(Some("cmd2 {={\narg1}=}"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("cmd1 '\ncmd2 arg'");
        assert_eq!(Some("cmd1 '"), commands.next());
        assert_eq!(Some("cmd2 arg'"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter(" #cmd1\ncmd2 arg #arg2\n \t #cmd3 arg\ncmd4 arg'");
        assert_eq!(Some("cmd2 arg #arg2"), commands.next());
        assert_eq!(Some("cmd4 arg'"), commands.next());
        assert_eq!(None, commands.next());
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

        fn assert_expansion(
            expected_expanded: &str,
            ctx: &EditorContext,
            text: &str,
        ) {
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
        assert_expansion("very long register contents short\0", &ctx, "{{@register(l) @register(s)}}");
        assert_expansion("short very long register contents\0", &ctx, "{{@register(s) @register(l)}}");

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
    }

    #[test]
    fn command_tokenizer() {
        let mut tokens = CommandTokenizer("cmd arg");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd 'arg0 \"arg1 ");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'arg0"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("\"arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg0'arg1 ");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg0'arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd arg0\"arg1 ");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg0\"arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd 'arg\"0' \"arg'1\"");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg\"0"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg'1"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd 'arg1\narg2'");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("'arg1"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg2'"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {{arg}}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {{%}%}=}}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("%}%}="), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd {=={arg}}=}}==}");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg}}=}"), tokens.next().map(|t| t.slice));
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

        let mut tokens = CommandTokenizer("@{={aa}=}");
        let token = tokens.next().unwrap();
        assert_eq!("aa", token.slice);
        assert!(!token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@@{={aa}=}");
        let token = tokens.next().unwrap();
        assert_eq!("aa", token.slice);
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
}

