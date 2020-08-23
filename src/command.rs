use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{Read, Write},
    ops::Range,
    path::Path,
    process::{Command, Stdio},
};

use crate::{
    buffer::{Buffer, BufferCollection, BufferContent, TextRef},
    buffer_view::{BufferView, BufferViewCollection, BufferViewHandle},
    config::{Config, ParseConfigError},
    connection::TargetClient,
    editor_operation::{EditorOperation, EditorOperationSerializer, StatusMessageKind},
    keymap::{KeyMapCollection, ParseKeyMapError},
    mode::Mode,
    pattern::Pattern,
    syntax::TokenKind,
    theme::ParseThemeError,
};

pub enum FullCommandOperation {
    Error,
    Complete,
    WaitForClient,
    Quit,
}

impl Default for FullCommandOperation {
    fn default() -> Self {
        Self::Error
    }
}

pub enum ConfigCommandOperation {
    Error,
    Complete,
}

impl Default for ConfigCommandOperation {
    fn default() -> Self {
        Self::Error
    }
}

pub struct FullCommandContext<'a> {
    pub target_client: TargetClient,
    pub operations: &'a mut EditorOperationSerializer,

    pub config: &'a Config,
    pub keymaps: &'a mut KeyMapCollection,
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub current_buffer_view_handle: &'a mut Option<BufferViewHandle>,
}

pub struct ConfigCommandContext<'a> {
    pub operations: &'a mut EditorOperationSerializer,
    pub config: &'a Config,
    pub keymaps: &'a mut KeyMapCollection,
}

type FullCommandBody = fn(
    &mut FullCommandContext,
    &mut CommandArgs,
    Option<&str>,
    &mut String,
) -> FullCommandOperation;
type ConfigCommandBody = fn(&mut ConfigCommandContext, &mut CommandArgs) -> ConfigCommandOperation;

struct ParsedCommand<'a> {
    pub name: &'a str,
    pub args: CommandArgs<'a>,
}

impl<'a> ParsedCommand<'a> {
    pub fn parse(command: &'a str) -> Option<Self> {
        let mut command = command.trim_start();
        match command.chars().next() {
            Some('|') => command = &command[1..].trim_start(),
            None => return None,
            _ => (),
        }

        if let Some(index) = command.find(' ') {
            Some(Self {
                name: &command[..index],
                args: CommandArgs::new(&command[index..]),
            })
        } else {
            Some(Self {
                name: command,
                args: CommandArgs::new(""),
            })
        }
    }

    pub fn unparsed(self) -> &'a str {
        self.args.raw
    }
}

pub struct CommandArgs<'a> {
    raw: &'a str,
}

impl<'a> CommandArgs<'a> {
    pub fn new(raw: &'a str) -> Self {
        Self { raw }
    }
}

impl<'a> Iterator for CommandArgs<'a> {
    type Item = Result<&'a str, String>;

    fn next(&mut self) -> Option<Self::Item> {
        fn find_string_end(s: &str, delim: char) -> Result<Range<usize>, String> {
            let mut chars = s.char_indices();
            chars.next();
            for (i, c) in chars {
                if c == delim {
                    return Ok(delim.len_utf8()..i);
                }
            }
            Err(format!("unclosed '{}'", delim))
        }

        self.raw = self.raw.trim_start();
        let (arg_range, arg_margin) = match self.raw.chars().next() {
            None | Some('|') => return None,
            Some(c @ '"') | Some(c @ '\'') => match find_string_end(self.raw, c) {
                Ok(range) => (range, c.len_utf8()),
                Err(error) => return Some(Err(error)),
            },
            _ => match self.raw.find(|c: char| c.is_whitespace()) {
                Some(end) => (0..end, 0),
                None => (0..self.raw.len(), 0),
            },
        };

        let (arg, after) = self.raw.split_at(arg_range.end + arg_margin);
        self.raw = after;

        Some(Ok(&arg[arg_range]))
    }
}

macro_rules! command_error {
    ($operations:expr, $error:expr) => {{
        $operations.serialize(
            TargetClient::All,
            &EditorOperation::StatusMessage(StatusMessageKind::Error, &$error),
        );
        return Default::default();
    }};
}

macro_rules! unwrap_or_command_error {
    ($operations:expr, $value:expr) => {
        match $value {
            Ok(value) => value,
            Err(error) => command_error!($operations, &error),
        }
    };
}

pub struct CommandCollection {
    full_commands: HashMap<String, FullCommandBody>,
    config_commands: HashMap<String, ConfigCommandBody>,
}

impl Default for CommandCollection {
    fn default() -> Self {
        let mut this = Self {
            full_commands: HashMap::new(),
            config_commands: HashMap::new(),
        };

        macro_rules! register {
            ($register_command:ident => $($name:ident,)*) => {
                $(this.$register_command(stringify!($name).replace('_', "-"), commands::$name);)*
            }
        }

        register! { register_full_command =>
            quit, open, close, save, save_all,
            selection, replace, echo, pipe, spawn,
        }

        register! { register_config_command =>
            set, syntax, theme,
            nmap, smap, imap,
        }

        this
    }
}

impl CommandCollection {
    pub fn register_full_command(&mut self, name: String, body: FullCommandBody) {
        self.full_commands.insert(name, body);
    }

    pub fn register_config_command(&mut self, name: String, body: ConfigCommandBody) {
        self.config_commands.insert(name, body);
    }

    pub fn parse_and_execut_config_command(
        &self,
        ctx: &mut ConfigCommandContext,
        command: &str,
    ) -> ConfigCommandOperation {
        let mut parsed = match ParsedCommand::parse(command) {
            Some(parsed) => parsed,
            None => command_error!(ctx.operations, "empty command name"),
        };

        if let Some(command) = self.config_commands.get(parsed.name) {
            command(ctx, &mut parsed.args)
        } else {
            command_error!(
                ctx.operations,
                format!("command '{}' not found", parsed.name)
            )
        }
    }

    pub fn parse_and_execute_any_command(
        &mut self,
        ctx: &mut FullCommandContext,
        mut command: &str,
    ) -> FullCommandOperation {
        let mut last_result = None;
        let mut input = String::new();
        let mut output = String::new();

        loop {
            let mut parsed = match ParsedCommand::parse(command) {
                Some(parsed) => parsed,
                None => {
                    break match last_result {
                        Some(result) => result,
                        None => command_error!(ctx.operations, "empty command name"),
                    }
                }
            };

            if let Some(command) = self.full_commands.get(parsed.name) {
                let maybe_input = match last_result {
                    Some(_) => Some(&input[..]),
                    None => None,
                };
                output.clear();
                last_result = match command(ctx, &mut parsed.args, maybe_input, &mut output) {
                    FullCommandOperation::Error => return FullCommandOperation::Error,
                    FullCommandOperation::WaitForClient => {
                        ctx.operations.serialize(
                            TargetClient::All,
                            &EditorOperation::Spawn(
                                parsed.unparsed().trim_start(),
                                maybe_input.unwrap_or(""),
                            ),
                        );
                        return FullCommandOperation::WaitForClient;
                    }
                    result => Some(result),
                };
                std::mem::swap(&mut input, &mut output);
            } else if let Some(command) = self.config_commands.get(parsed.name) {
                let mut ctx = ConfigCommandContext {
                    operations: ctx.operations,
                    config: ctx.config,
                    keymaps: ctx.keymaps,
                };
                if let ConfigCommandOperation::Error = command(&mut ctx, &mut parsed.args) {
                    return FullCommandOperation::Error;
                }
                last_result = Some(FullCommandOperation::Complete);
                input.clear();
            } else {
                command_error!(
                    ctx.operations,
                    format!("command '{}' not found", parsed.name)
                );
            }

            command = parsed.unparsed();
        }
    }
}

macro_rules! assert_empty {
    ($operations:expr, $args:expr) => {
        match $args.next() {
            Some(_) => command_error!($operations, "command expected less arguments"),
            None => (),
        }
    };
}

macro_rules! expect_next {
    ($operations:expr, $args:expr) => {
        match $args.next() {
            Some(Ok(arg)) => arg,
            Some(Err(error)) => command_error!($operations, &error),
            None => command_error!($operations, "command expected more arguments"),
        }
    };
}

macro_rules! input_or_next {
    ($operations:expr, $args:expr, $input:expr) => {
        match $input {
            Some(input) => Some(input),
            None => match $args.next() {
                Some(Ok(arg)) => Some(arg),
                Some(Err(error)) => command_error!($operations, &error),
                None => None,
            },
        }
    };
}

macro_rules! expect_input_or_next {
    ($operations:expr, $args:expr, $input:expr) => {
        match $input {
            Some(input) => input,
            None => expect_next!($operations, $args),
        }
    };
}

mod helper {
    use super::*;

    pub fn parsing_error<T>(message: T, text: &str, error_index: usize) -> String
    where
        T: fmt::Display,
    {
        let (before, after) = text.split_at(error_index);
        match (before.len(), after.len()) {
            (0, 0) => format!("{} at ''", message),
            (_, 0) => format!("{} at '{}' <- here", message, before),
            (0, _) => format!("{} at here -> '{}'", message, after),
            (_, _) => format!("{} at '{}' <- here '{}'", message, before, after),
        }
    }

    pub fn new_buffer_from_content(
        ctx: &mut FullCommandContext,
        path: &Path,
        content: BufferContent,
    ) {
        ctx.operations.serialize_buffer(ctx.target_client, &content);
        ctx.operations
            .serialize(ctx.target_client, &EditorOperation::Path(path));

        let buffer_handle = ctx.buffers.add(Buffer::new(path.into(), content));
        let buffer_view = BufferView::new(ctx.target_client, buffer_handle);
        let buffer_view_handle = ctx.buffer_views.add(buffer_view);
        *ctx.current_buffer_view_handle = Some(buffer_view_handle);
    }

    pub fn new_buffer_from_file(ctx: &mut FullCommandContext, path: &Path) -> Result<(), String> {
        if let Some(buffer_handle) = ctx.buffers.find_with_path(path) {
            let mut iter = ctx
                .buffer_views
                .iter_with_handles()
                .filter_map(|(handle, view)| {
                    if view.buffer_handle == buffer_handle
                        && view.target_client == ctx.target_client
                    {
                        Some((handle, view))
                    } else {
                        None
                    }
                });

            let view = match iter.next() {
                Some((handle, view)) => {
                    *ctx.current_buffer_view_handle = Some(handle);
                    view
                }
                None => {
                    drop(iter);
                    let view = BufferView::new(ctx.target_client, buffer_handle);
                    let view_handle = ctx.buffer_views.add(view);
                    let view = ctx.buffer_views.get(&view_handle);
                    *ctx.current_buffer_view_handle = Some(view_handle);
                    view
                }
            };

            ctx.operations.serialize_buffer(
                ctx.target_client,
                &ctx.buffers.get(buffer_handle).unwrap().content,
            );
            ctx.operations
                .serialize(ctx.target_client, &EditorOperation::Path(path));
            ctx.operations
                .serialize_cursors(ctx.target_client, &view.cursors);
        } else if path.to_str().map(|s| s.trim().len()).unwrap_or(0) > 0 {
            let content = match File::open(&path) {
                Ok(mut file) => {
                    let mut content = String::new();
                    match file.read_to_string(&mut content) {
                        Ok(_) => (),
                        Err(error) => {
                            return Err(format!(
                                "could not read contents from file {:?}: {:?}",
                                path, error
                            ))
                        }
                    }
                    BufferContent::from_str(&content[..])
                }
                Err(_) => BufferContent::from_str(""),
            };

            new_buffer_from_content(ctx, path, content);
        } else {
            return Err(format!("invalid path {:?}", path));
        }

        Ok(())
    }

    pub fn write_buffer_to_file(buffer: &Buffer, path: &Path) -> Result<(), String> {
        let mut file =
            File::create(path).map_err(|e| format!("could not create file {:?}: {:?}", path, e))?;

        buffer
            .content
            .write(&mut file)
            .map_err(|e| format!("could not write to file {:?}: {:?}", path, e))
    }
}

mod commands {
    use super::*;

    pub fn quit(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        _input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        assert_empty!(ctx.operations, args);
        FullCommandOperation::Quit
    }

    pub fn open<'a, 'b>(
        mut ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        let path = Path::new(expect_input_or_next!(ctx.operations, args, input));
        assert_empty!(ctx.operations, args);
        if let Err(error) = helper::new_buffer_from_file(&mut ctx, path) {
            command_error!(ctx.operations, error);
        }
        FullCommandOperation::Complete
    }

    pub fn close(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        _input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        assert_empty!(ctx.operations, args);
        if let Some(handle) = ctx
            .current_buffer_view_handle
            .take()
            .map(|h| ctx.buffer_views.get(&h).buffer_handle)
        {
            for view in ctx.buffer_views.iter() {
                if view.buffer_handle == handle {
                    ctx.operations
                        .serialize(view.target_client, &EditorOperation::Buffer(""));
                    ctx.operations
                        .serialize(view.target_client, &EditorOperation::Path(Path::new("")));
                }
            }
            ctx.buffer_views
                .remove_where(|view| view.buffer_handle == handle);
        }

        FullCommandOperation::Complete
    }

    pub fn save(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        let view_handle = match ctx.current_buffer_view_handle.as_ref() {
            Some(handle) => handle,
            None => command_error!(ctx.operations, "no buffer opened"),
        };

        let buffer_handle = ctx.buffer_views.get(view_handle).buffer_handle;
        let buffer = match ctx.buffers.get_mut(buffer_handle) {
            Some(buffer) => buffer,
            None => command_error!(ctx.operations, "no buffer opened"),
        };

        let path = input_or_next!(ctx.operations, args, input);
        assert_empty!(ctx.operations, args);
        match path {
            Some(path) => {
                let path = Path::new(path);
                if let Err(error) = helper::write_buffer_to_file(buffer, path) {
                    command_error!(ctx.operations, error);
                }
                for view in ctx.buffer_views.iter() {
                    if view.buffer_handle == buffer_handle {
                        ctx.operations
                            .serialize(view.target_client, &EditorOperation::Path(path));
                    }
                }
                buffer.path.clear();
                buffer.path.push(path);
                FullCommandOperation::Complete
            }
            None => {
                if !buffer.path.as_os_str().is_empty() {
                    command_error!(ctx.operations, "buffer has no path");
                }
                if let Err(error) = helper::write_buffer_to_file(buffer, &buffer.path) {
                    command_error!(ctx.operations, error);
                }
                FullCommandOperation::Complete
            }
        }
    }

    pub fn save_all(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        _input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        assert_empty!(ctx.operations, args);
        for buffer in ctx.buffers.iter() {
            if !buffer.path.as_os_str().is_empty() {
                if let Err(error) = helper::write_buffer_to_file(buffer, &buffer.path) {
                    command_error!(ctx.operations, error);
                }
            }
        }

        FullCommandOperation::Complete
    }

    pub fn selection(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        _input: Option<&str>,
        output: &mut String,
    ) -> FullCommandOperation {
        assert_empty!(ctx.operations, args);
        if let Some(buffer_view) = ctx
            .current_buffer_view_handle
            .as_ref()
            .map(|h| ctx.buffer_views.get(h))
        {
            buffer_view.get_selection_text(ctx.buffers, output);
        }

        FullCommandOperation::Complete
    }

    pub fn replace(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        let input = expect_input_or_next!(ctx.operations, args, input);
        assert_empty!(ctx.operations, args);
        if let Some(handle) = ctx.current_buffer_view_handle {
            ctx.buffer_views
                .delete_in_selection(ctx.buffers, ctx.operations, handle);
            ctx.buffer_views
                .insert_text(ctx.buffers, ctx.operations, handle, TextRef::Str(input));
        }

        FullCommandOperation::Complete
    }

    pub fn echo(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        ctx.operations.serialize(
            TargetClient::All,
            &EditorOperation::StatusMessage(StatusMessageKind::Info, ""),
        );

        if let Some(input) = input {
            ctx.operations.serialize(
                TargetClient::All,
                &EditorOperation::StatusMessageAppend(input),
            );
            ctx.operations.serialize(
                TargetClient::All,
                &EditorOperation::StatusMessageAppend(" "),
            );
        }

        for arg in args {
            let arg = unwrap_or_command_error!(ctx.operations, arg);
            ctx.operations.serialize(
                TargetClient::All,
                &EditorOperation::StatusMessageAppend(arg),
            );
            ctx.operations.serialize(
                TargetClient::All,
                &EditorOperation::StatusMessageAppend(" "),
            );
        }

        FullCommandOperation::Complete
    }

    pub fn pipe(
        ctx: &mut FullCommandContext,
        args: &mut CommandArgs,
        input: Option<&str>,
        output: &mut String,
    ) -> FullCommandOperation {
        let name = expect_next!(ctx.operations, args);

        let mut command = Command::new(name);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        for arg in args {
            let arg = unwrap_or_command_error!(ctx.operations, arg);
            command.arg(arg);
        }

        let mut child =
            unwrap_or_command_error!(ctx.operations, command.spawn().map_err(|e| e.to_string()));
        if let (Some(input), Some(stdin)) = (input, child.stdin.as_mut()) {
            let _ = stdin.write_all(input.as_bytes());
        }
        child.stdin = None;

        let child_output = unwrap_or_command_error!(
            ctx.operations,
            child.wait_with_output().map_err(|e| e.to_string())
        );
        if child_output.status.success() {
            let child_output = String::from_utf8_lossy(&child_output.stdout[..]);
            output.push_str(child_output.as_ref());
        } else {
            let child_output = String::from_utf8_lossy(&child_output.stdout[..]);
            command_error!(ctx.operations, child_output);
        }

        FullCommandOperation::Complete
    }

    pub fn spawn(
        _ctx: &mut FullCommandContext,
        _args: &mut CommandArgs,
        _input: Option<&str>,
        _output: &mut String,
    ) -> FullCommandOperation {
        FullCommandOperation::WaitForClient
    }

    pub fn set(ctx: &mut ConfigCommandContext, args: &mut CommandArgs) -> ConfigCommandOperation {
        let name = expect_next!(ctx.operations, args);
        let mut previous = "";
        let mut args = args.map(|arg| {
            if let Ok(arg) = arg {
                previous = arg
            }
            arg
        });

        let mut values = ctx.config.values.clone();
        match values.parse_and_set(name, &mut args) {
            Ok(()) => assert_empty!(ctx.operations, args),
            Err(e) => {
                let message = match e {
                    ParseConfigError::ConfigNotFound => helper::parsing_error(e, name, 0),
                    ParseConfigError::ParseError(e) => helper::parsing_error(e, previous, 0),
                    ParseConfigError::UnexpectedEndOfValues => {
                        helper::parsing_error(e, previous, previous.len())
                    }
                };
                command_error!(ctx.operations, message);
            }
        }

        ctx.operations
            .serialize_config_values(TargetClient::All, &values);
        ConfigCommandOperation::Complete
    }

    pub fn syntax(
        ctx: &mut ConfigCommandContext,
        args: &mut CommandArgs,
    ) -> ConfigCommandOperation {
        let main_extension = expect_next!(ctx.operations, args);
        let subcommand = expect_next!(ctx.operations, args);
        if subcommand == "extension" {
            for extension in args {
                let extension = unwrap_or_command_error!(ctx.operations, extension);
                ctx.operations.serialize(
                    TargetClient::All,
                    &EditorOperation::SyntaxExtension(main_extension, extension),
                );
            }
        } else if let Some(token_kind) = TokenKind::from_str(subcommand) {
            for pattern in args {
                let pattern = unwrap_or_command_error!(ctx.operations, pattern);
                let pattern = match Pattern::new(pattern) {
                    Ok(pattern) => pattern,
                    Err(error) => {
                        let message = helper::parsing_error(error, pattern, 0);
                        command_error!(ctx.operations, message);
                    }
                };
                ctx.operations.serialize_syntax_rule(
                    TargetClient::All,
                    main_extension,
                    token_kind,
                    &pattern,
                );
            }
        } else {
            command_error!(
                ctx.operations,
                format!(
                    "no such subcommand '{}'. expected either 'extension' or a token kind",
                    subcommand
                )
            );
        }

        ConfigCommandOperation::Complete
    }

    pub fn theme(ctx: &mut ConfigCommandContext, args: &mut CommandArgs) -> ConfigCommandOperation {
        let name = expect_next!(ctx.operations, args);
        let color = expect_next!(ctx.operations, args);
        assert_empty!(ctx.operations, args);

        let mut theme = ctx.config.theme.clone();
        if let Err(e) = theme.parse_and_set(name, color) {
            let context = format!("{} {}", name, color);
            let error_index = match e {
                ParseThemeError::ColorNotFound => 0,
                _ => context.len(),
            };

            command_error!(
                ctx.operations,
                helper::parsing_error(e, &context[..], error_index)
            );
        }

        ctx.operations.serialize_theme(TargetClient::All, &theme);
        ConfigCommandOperation::Complete
    }

    pub fn nmap(ctx: &mut ConfigCommandContext, args: &mut CommandArgs) -> ConfigCommandOperation {
        mode_map(ctx, args, Mode::Normal)
    }

    pub fn smap(ctx: &mut ConfigCommandContext, args: &mut CommandArgs) -> ConfigCommandOperation {
        mode_map(ctx, args, Mode::Select)
    }

    pub fn imap(ctx: &mut ConfigCommandContext, args: &mut CommandArgs) -> ConfigCommandOperation {
        mode_map(ctx, args, Mode::Insert)
    }

    fn mode_map(
        ctx: &mut ConfigCommandContext,
        args: &mut CommandArgs,
        mode: Mode,
    ) -> ConfigCommandOperation {
        let from = expect_next!(ctx.operations, args);
        let to = expect_next!(ctx.operations, args);
        assert_empty!(ctx.operations, args);

        match ctx.keymaps.parse_map(mode.discriminant(), from, to) {
            Ok(()) => (),
            Err(ParseKeyMapError::From(i, e)) => {
                command_error!(ctx.operations, helper::parsing_error(e, from, i))
            }
            Err(ParseKeyMapError::To(i, e)) => {
                command_error!(ctx.operations, helper::parsing_error(e, to, i))
            }
        }

        ConfigCommandOperation::Complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_command_arg_parsing() {
        let mut args = CommandArgs::new("arg");
        assert_eq!(Some(Ok("arg")), args.next());
        assert_eq!(None, args.next());

        let mut args = CommandArgs::new("  ' arg ' ");
        assert_eq!(Some(Ok(" arg ")), args.next());
        assert_eq!(None, args.next());

        let mut args = CommandArgs::new("  \" arg \" ");
        assert_eq!(Some(Ok(" arg ")), args.next());
        assert_eq!(None, args.next());
    }

    #[test]
    fn multiple_command_arg_parsing() {
        let mut args = CommandArgs::new("arg1 arg2");
        assert_eq!(Some(Ok("arg1")), args.next());
        assert_eq!(Some(Ok("arg2")), args.next());
        assert_eq!(None, args.next());

        let mut args = CommandArgs::new("  ' arg1 '   '  arg2' ");
        assert_eq!(Some(Ok(" arg1 ")), args.next());
        assert_eq!(Some(Ok("  arg2")), args.next());
        assert_eq!(None, args.next());

        let mut args = CommandArgs::new("  \" arg \" ");
        assert_eq!(Some(Ok(" arg ")), args.next());
        assert_eq!(None, args.next());
    }

    #[test]
    fn fail_arg_parsing() {
        let mut args = CommandArgs::new("'arg");
        assert!(args.next().unwrap().is_err());

        let mut args = CommandArgs::new("\"arg");
        assert!(args.next().unwrap().is_err());
    }

    #[test]
    fn single_command_parsing() {
        let mut parsed = ParsedCommand::parse("name arg1 arg2").unwrap();
        assert_eq!("name", parsed.name);
        assert_eq!(Some(Ok("arg1")), parsed.args.next());
        assert_eq!(Some(Ok("arg2")), parsed.args.next());
        assert_eq!(None, parsed.args.next());
        assert!(parsed.unparsed().trim().is_empty());

        let mut parsed = ParsedCommand::parse("name   'arg1 '   \" arg2 '\"").unwrap();
        assert_eq!("name", parsed.name);
        assert_eq!(Some(Ok("arg1 ")), parsed.args.next());
        assert_eq!(Some(Ok(" arg2 '")), parsed.args.next());
        assert_eq!(None, parsed.args.next());
        assert!(parsed.unparsed().trim().is_empty());
    }

    #[test]
    fn multiple_command_parsing() {
        let mut parsed = ParsedCommand::parse("name1 arg1 arg2 |    name2 arg3 arg4").unwrap();
        assert_eq!("name1", parsed.name);
        assert_eq!(Some(Ok("arg1")), parsed.args.next());
        assert_eq!(Some(Ok("arg2")), parsed.args.next());
        assert_eq!(None, parsed.args.next());

        let mut parsed = ParsedCommand::parse(parsed.unparsed()).unwrap();
        assert_eq!("name2", parsed.name);
        assert_eq!(Some(Ok("arg3")), parsed.args.next());
        assert_eq!(Some(Ok("arg4")), parsed.args.next());
        assert_eq!(None, parsed.args.next());
        assert!(parsed.unparsed().trim().is_empty());

        let mut parsed = ParsedCommand::parse("name1 'arg1 |  name2'").unwrap();
        assert_eq!("name1", parsed.name);
        assert_eq!(Some(Ok("arg1 |  name2")), parsed.args.next());
        assert_eq!(None, parsed.args.next());
        assert!(parsed.unparsed().trim().is_empty());
    }
}
