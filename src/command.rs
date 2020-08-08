use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
};

use crate::{
    buffer::{Buffer, BufferCollection, BufferContent},
    buffer_view::{BufferView, BufferViewCollection, BufferViewHandle},
    config::Config,
    connection::TargetClient,
    editor::{EditorOperation, EditorOperationSender},
    keymap::{KeyMapCollection, ParseKeyMapError},
    mode::Mode,
    pattern::Pattern,
    syntax::{Syntax, TokenKind},
};

type CommandResult = Result<CommandOperation, String>;

pub enum CommandOperation {
    Complete,
    Quit,
}

pub struct CommandContext<'a> {
    pub target_client: TargetClient,
    pub operations: &'a mut EditorOperationSender,

    pub config: &'a mut Config,
    pub keymaps: &'a mut KeyMapCollection,
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub current_buffer_view_handle: &'a mut Option<BufferViewHandle>,
}

type CommandBody = fn(CommandContext, CommandArgs) -> CommandResult;

pub struct CommandArgs<'a> {
    raw: &'a str,
}

impl<'a> CommandArgs<'a> {
    pub fn new(args: &'a str) -> Self {
        Self { raw: args }
    }

    pub fn assert_empty(&self) -> Result<(), String> {
        if self.raw.trim_start().len() > 0 {
            Err("command expected less arguments".into())
        } else {
            Ok(())
        }
    }

    pub fn expect_next(&mut self) -> Result<&'a str, String> {
        self.next()
            .ok_or_else(|| String::from("command expected more arguments"))
    }
}

impl<'a> Iterator for CommandArgs<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        fn find_string_end(s: &str, delim: char) -> Option<Range<usize>> {
            let mut chars = s.char_indices();
            chars.next()?;
            for (i, c) in chars {
                if c == delim {
                    return Some(delim.len_utf8()..i);
                }
            }
            None
        }

        self.raw = self.raw.trim_start();
        if self.raw.len() == 0 {
            return None;
        }

        let arg_range = match self.raw.chars().next() {
            Some('"') => find_string_end(self.raw, '"')?,
            Some('\'') => find_string_end(self.raw, '\'')?,
            _ => match self.raw.find(|c: char| c.is_whitespace()) {
                Some(end) => 0..end,
                None => 0..self.raw.len(),
            },
        };

        let (arg, after) = self.raw.split_at(arg_range.end);
        self.raw = after;

        Some(&arg[arg_range])
    }
}

pub struct CommandCollection {
    commands: HashMap<String, CommandBody>,
}

impl Default for CommandCollection {
    fn default() -> Self {
        let mut this = Self {
            commands: HashMap::new(),
        };

        macro_rules! register_all {
            ($($name:ident,)*) => {
                $(this.register(stringify!($name).replace('_', "-"), commands::$name);)*
            }
        }

        register_all! {
            quit, edit, close, write, write_all,
            set, syntax,
            nmap, smap, imap,
        }

        this
    }
}

impl CommandCollection {
    pub fn register(&mut self, name: String, body: CommandBody) {
        self.commands.insert(name, body);
    }

    pub fn parse_and_execute(&self, ctx: CommandContext, command: &str) -> CommandResult {
        let command = command.trim();
        let name;
        let args;
        if let Some(index) = command.find(' ') {
            name = &command[..index];
            args = CommandArgs::new(&command[(index + 1)..]);
        } else {
            name = command;
            args = CommandArgs::new("");
        }

        if let Some(command) = self.commands.get(name) {
            command(ctx, args)
        } else {
            Err(format!("command '{}' not found", name))
        }
    }
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
        ctx: &mut CommandContext,
        path: Option<PathBuf>,
        content: BufferContent,
    ) {
        ctx.operations.send_content(ctx.target_client, &content);
        ctx.operations
            .send(ctx.target_client, EditorOperation::Path(path.clone()));

        let buffer_handle = ctx.buffers.add(Buffer::new(path, content));
        let buffer_view = BufferView::new(ctx.target_client, buffer_handle);
        let buffer_view_handle = ctx.buffer_views.add(buffer_view);
        *ctx.current_buffer_view_handle = Some(buffer_view_handle);
    }

    pub fn new_buffer_from_file(ctx: &mut CommandContext, path: &Path) -> Result<(), String> {
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

            ctx.operations.send_content(
                ctx.target_client,
                &ctx.buffers.get(buffer_handle).unwrap().content,
            );
            ctx.operations
                .send(ctx.target_client, EditorOperation::Path(Some(path.into())));
            ctx.operations
                .send_cursors(ctx.target_client, &view.cursors);
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

            new_buffer_from_content(ctx, Some(path.into()), content);
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

    pub fn quit(_ctx: CommandContext, args: CommandArgs) -> CommandResult {
        args.assert_empty()?;
        Ok(CommandOperation::Quit)
    }

    pub fn edit(mut ctx: CommandContext, mut args: CommandArgs) -> CommandResult {
        let path = Path::new(args.expect_next()?);
        args.assert_empty()?;
        helper::new_buffer_from_file(&mut ctx, path)?;
        Ok(CommandOperation::Complete)
    }

    pub fn close(ctx: CommandContext, args: CommandArgs) -> CommandResult {
        args.assert_empty()?;
        if let Some(handle) = ctx
            .current_buffer_view_handle
            .take()
            .map(|h| ctx.buffer_views.get(&h).buffer_handle)
        {
            for view in ctx.buffer_views.iter() {
                if view.buffer_handle == handle {
                    ctx.operations.send_empty_content(view.target_client);
                    ctx.operations
                        .send(view.target_client, EditorOperation::Path(None));
                }
            }
            ctx.buffer_views
                .remove_where(|view| view.buffer_handle == handle);
        }

        Ok(CommandOperation::Complete)
    }

    pub fn write(ctx: CommandContext, mut args: CommandArgs) -> CommandResult {
        let view_handle = ctx
            .current_buffer_view_handle
            .as_ref()
            .ok_or_else(|| String::from("no buffer opened"))?;

        let buffer_handle = ctx.buffer_views.get(view_handle).buffer_handle;
        let buffer = ctx
            .buffers
            .get_mut(buffer_handle)
            .ok_or_else(|| String::from("no buffer opened"))?;

        let path = args.next();
        args.assert_empty()?;
        match path {
            Some(path) => {
                let path = PathBuf::from(path);
                helper::write_buffer_to_file(buffer, &path)?;
                for view in ctx.buffer_views.iter() {
                    if view.buffer_handle == buffer_handle {
                        ctx.operations.send(
                            view.target_client,
                            EditorOperation::Path(Some(path.clone())),
                        );
                    }
                }
                buffer.path = Some(path.clone());
                Ok(CommandOperation::Complete)
            }
            None => {
                let path = buffer
                    .path
                    .as_ref()
                    .ok_or_else(|| String::from("buffer has no path"))?;
                helper::write_buffer_to_file(buffer, path)?;
                Ok(CommandOperation::Complete)
            }
        }
    }

    pub fn write_all(ctx: CommandContext, args: CommandArgs) -> CommandResult {
        args.assert_empty()?;
        for buffer in ctx.buffers.iter() {
            if let Some(ref path) = buffer.path {
                helper::write_buffer_to_file(buffer, path)?;
            }
        }

        Ok(CommandOperation::Complete)
    }

    pub fn set(ctx: CommandContext, mut args: CommandArgs) -> CommandResult {
        let name = args.expect_next()?;
        ctx.config.parse_and_set(name, args)?;
        Ok(CommandOperation::Complete)
    }

    pub fn syntax(ctx: CommandContext, mut args: CommandArgs) -> CommandResult {
        let extension = args.expect_next()?;
        let handle = match ctx.config.syntaxes.find_by_extension(extension) {
            Some(handle) => handle,
            None => ctx
                .config
                .syntaxes
                .add(Syntax::with_extension(extension.into())),
        };
        let syntax = ctx.config.syntaxes.get_mut(handle);

        let subcommand = args.expect_next()?;
        if subcommand == "extension" {
            for extension in args {
                syntax.add_extension(extension.into());
            }
        } else if let Some(token_kind) = TokenKind::from_str(subcommand) {
            for pattern in args {
                syntax.add_rule(
                    token_kind,
                    Pattern::new(pattern).map_err(|e| helper::parsing_error(e, pattern, 0))?,
                );
            }
        } else {
            return Err(format!(
                "no such subcommand '{}'. expected either 'extension' or a token kind",
                subcommand
            ));
        }

        Ok(CommandOperation::Complete)
    }

    pub fn nmap(ctx: CommandContext, args: CommandArgs) -> CommandResult {
        mode_map(ctx, args, Mode::Normal)
    }

    pub fn smap(ctx: CommandContext, args: CommandArgs) -> CommandResult {
        mode_map(ctx, args, Mode::Select)
    }

    pub fn imap(ctx: CommandContext, args: CommandArgs) -> CommandResult {
        mode_map(ctx, args, Mode::Insert)
    }

    fn mode_map(ctx: CommandContext, mut args: CommandArgs, mode: Mode) -> CommandResult {
        let from = args.expect_next()?;
        let to = args.expect_next()?;
        args.assert_empty()?;

        match ctx.keymaps.parse_map(mode.discriminant(), from, to) {
            Ok(()) => Ok(CommandOperation::Complete),
            Err(ParseKeyMapError::From(i, e)) => Err(helper::parsing_error(e, from, i)),
            Err(ParseKeyMapError::To(i, e)) => Err(helper::parsing_error(e, to, i)),
        }
    }
}
