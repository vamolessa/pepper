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

pub struct CommandArgs<'command>(CommandTokenizer<'command>);
impl<'command> CommandArgs<'command> {
    pub fn try_next(&mut self) -> Option<&'command str> {
        self.0.next().map(|t| t.slice)
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
            match s.find(&[' ', '\t'][..]) {
                Some(i) => i,
                None => s.len(),
            }
        }

        fn parse_balanced_token(s: &str) -> Option<(&str, &str)> {
            let mut chars = s.chars();
            let mut depth = 0;
            loop {
                match chars.next()? {
                    '=' => depth += 1,
                    '[' => break,
                    _ => return None,
                }
            }
            let start = chars.as_str().as_ptr() as usize;
            let mut end = start;
            let mut ending = false;
            let mut matched = 0;
            loop {
                match chars.next()? {
                    ']' => {
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

        self.0 = self.0.trim_start_matches(&[' ', '\t'][..]);

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
                    match rest.find(delim) {
                        Some(i) => {
                            let slice = &rest[..i];
                            self.0 = &rest[i + 1..];
                            return Some(CommandToken {
                                can_expand_variables,
                                slice,
                            })
                        }
                        None => {
                            let end = next_literal_end(rest);
                            let (slice, rest) = self.0.split_at(end + 1);
                            self.0 = rest;
                            return Some(CommandToken {
                                can_expand_variables,
                                slice,
                            })
                        }
                    }
                }
                c => {
                    if c == '[' {
                        if let Some((slice, rest)) = parse_balanced_token(&self.0[1..]) {
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
                    })
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
        command: &mut String,
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
        command: &mut String,
    ) -> Result<EditorFlow, CommandError> {
        expand_variables(ctx, command);

        if let Some(token) = CommandTokenizer(command).next() {
            let alias = token.slice.trim_end_matches('!');
            if let Some(aliased) = ctx.editor.commands.aliases.find(alias) {
                let start = alias.as_ptr() as usize - command.as_ptr() as usize;
                let end = start + alias.len();
                command.replace_range(start..end, aliased);
            }
        }

        Self::eval(ctx, client_handle, command)
    }

    fn eval(
        ctx: &mut EditorContext,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> Result<EditorFlow, CommandError> {
        let mut tokenizer = CommandTokenizer(command);
        let command = match tokenizer.next() {
            Some(command) => command.slice,
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
            args: CommandArgs(tokenizer),
            bang,
            flow: EditorFlow::Continue,
        };
        command_fn(ctx, &mut io)?;
        Ok(io.flow)
    }
}

fn expand_variables(_ctx: &EditorContext, text: &mut String) {
    let text_ptr = text.as_ptr() as usize;

    let mut i = 0;
    loop {
        let mut tokens = CommandTokenizer(&text[i..]);
        let token = match tokens.next() {
            Some(token) => token,
            None => return,
        };
        i = tokens.0.as_ptr() as usize - text_ptr;

        if !token.can_expand_variables {
            continue;
        }

        let expanded = match token.slice {
            "@buffer_path" => "example/buffer/path",
            _ => continue,
        };

        let token_len = token.slice.len();
        let token_start = token.slice.as_ptr() as usize - text_ptr;
        let token_end = token_start + token_len;

        if expanded.len() < token_len {
            i -= token_len - expanded.len();
        } else {
            i += token_len - expanded.len();
        }

        text.replace_range(token_start..token_end, expanded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_tokens() {
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

        let mut tokens = CommandTokenizer("cmd [[arg]]");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg"), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd [[%]%]=]]");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("%]%]="), tokens.next().map(|t| t.slice));
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("cmd [==[arg]]=]]==]");
        assert_eq!(Some("cmd"), tokens.next().map(|t| t.slice));
        assert_eq!(Some("arg]]=]"), tokens.next().map(|t| t.slice));
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

        let mut tokens = CommandTokenizer("@[=[aa]=]");
        let token = tokens.next().unwrap();
        assert_eq!("aa", token.slice);
        assert!(!token.can_expand_variables);
        assert_eq!(None, tokens.next().map(|t| t.slice));

        let mut tokens = CommandTokenizer("@@[=[aa]=]");
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

