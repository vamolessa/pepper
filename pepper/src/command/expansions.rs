use std::{env, fmt, path::Path};

use crate::{
    buffer::{Buffer, BufferHandle},
    buffer_view::BufferView,
    client::ClientHandle,
    command::CommandArgs,
    cursor::Cursor,
    editor::EditorContext,
    editor_utils::RegisterKey,
};

pub enum ExpansionError {
    IgnoreExpansion,
    ArgumentNotEmpty,
    InvalidArgIndex,
    InvalidBufferId,
    InvalidCursorIndex,
    InvalidRegisterKey,
}
impl fmt::Display for ExpansionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::IgnoreExpansion => unreachable!(),
            Self::ArgumentNotEmpty => f.write_str("argument not empty"),
            Self::InvalidArgIndex => f.write_str("invalid arg index"),
            Self::InvalidBufferId => f.write_str("invalid buffer id"),
            Self::InvalidCursorIndex => f.write_str("invalid cursor index"),
            Self::InvalidRegisterKey => f.write_str("invalid register key"),
        }
    }
}

pub fn write_variable_expansion<'ctx>(
    ctx: &'ctx EditorContext,
    client_handle: Option<ClientHandle>,
    mut command_args: CommandArgs,
    command_bang: bool,
    name: &str,
    args: &str,
    output: &mut String,
) -> Result<(), ExpansionError> {
    use fmt::Write;

    match name {
        "arg" => match args {
            "!" => {
                if command_bang {
                    output.push('!');
                }
            }
            "*" => {
                let args = match command_args.0.strip_suffix('\0') {
                    Some(args) => args,
                    None => return Err(ExpansionError::IgnoreExpansion),
                };
                output.push_str(args);
            }
            _ => {
                let mut index: usize = args.parse().map_err(|_| ExpansionError::InvalidArgIndex)?;
                while let Some(arg) = command_args.try_next() {
                    if index == 0 {
                        output.push_str(arg);
                        break;
                    }
                    index -= 1;
                }
            }
        },
        "client-id" => {
            assert_empty_args(args)?;
            if let Some(client_handle) = client_handle {
                let _ = write!(output, "{}", client_handle.0);
            }
        }
        "buffer-id" => {
            assert_empty_args(args)?;
            if let Some(buffer) = current_buffer(ctx, client_handle) {
                let _ = write!(output, "{}", buffer.handle().0);
            }
        }
        "buffer-path" => {
            let buffer = if args.is_empty() {
                current_buffer(ctx, client_handle)
            } else {
                let id = args.parse().map_err(|_| ExpansionError::InvalidBufferId)?;
                ctx.editor.buffers.try_get(BufferHandle(id))
            };
            if let Some(path) = buffer.and_then(|b| b.path.to_str()) {
                output.push_str(path);
            }
        }
        "buffer-absolute-path" => {
            let buffer = if args.is_empty() {
                current_buffer(ctx, client_handle)
            } else {
                let id = args.parse().map_err(|_| ExpansionError::InvalidBufferId)?;
                ctx.editor.buffers.try_get(BufferHandle(id))
            };
            let current_directory = ctx.editor.current_directory.to_str();
            let path = buffer.and_then(|b| b.path.to_str());
            if let (Some(current_directory), Some(path)) = (current_directory, path) {
                if Path::new(path).is_relative() {
                    output.push_str(current_directory);
                    if let Some(false) = current_directory
                        .chars()
                        .next_back()
                        .map(std::path::is_separator)
                    {
                        output.push(std::path::MAIN_SEPARATOR);
                    }
                }
                output.push_str(path);
            }
        }
        "buffer-content" => {
            let buffer = if args.is_empty() {
                current_buffer(ctx, client_handle)
            } else {
                let id = args.parse().map_err(|_| ExpansionError::InvalidBufferId)?;
                let handle = BufferHandle(id);
                ctx.editor.buffers.try_get(handle)
            };
            if let Some(buffer) = buffer {
                for line in buffer.content().lines() {
                    output.push_str(line.as_str());
                    output.push('\n');
                }
            }
        }
        "cursor-anchor-column" => {
            if let Some(cursor) = cursor(ctx, client_handle, args)? {
                let _ = write!(output, "{}", cursor.anchor.column_byte_index);
            }
        }
        "cursor-anchor-line" => {
            if let Some(cursor) = cursor(ctx, client_handle, args)? {
                let _ = write!(output, "{}", cursor.anchor.line_index);
            }
        }
        "cursor-position-column" => {
            if let Some(cursor) = cursor(ctx, client_handle, args)? {
                let _ = write!(output, "{}", cursor.position.column_byte_index);
            }
        }
        "cursor-position-line" => {
            if let Some(cursor) = cursor(ctx, client_handle, args)? {
                let _ = write!(output, "{}", cursor.position.line_index);
            }
        }
        "cursor-selection" => {
            let buffer = current_buffer(ctx, client_handle);
            let cursor = cursor(ctx, client_handle, args)?;
            if let (Some(buffer), Some(cursor)) = (buffer, cursor) {
                let range = cursor.to_range();
                for text in buffer.content().text_range(range) {
                    output.push_str(text);
                }
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
            let key = RegisterKey::from_str(args).ok_or(ExpansionError::InvalidRegisterKey)?;
            output.push_str(ctx.editor.registers.get(key));
        }
        "env" => {
            if let Ok(env_var) = env::var(args) {
                output.push_str(&env_var);
            }
        }
        "pid" => {
            assert_empty_args(args)?;
            let _ = write!(output, "{}", std::process::id());
        }
        _ => (),
    }

    Ok(())
}

fn assert_empty_args(args: &str) -> Result<(), ExpansionError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(ExpansionError::ArgumentNotEmpty)
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
) -> Result<Option<Cursor>, ExpansionError> {
    let cursors = match current_buffer_view(ctx, client_handle) {
        Some(view) => &view.cursors,
        None => return Ok(None),
    };
    let index = if args.is_empty() {
        cursors.main_cursor_index()
    } else {
        args.parse()
            .map_err(|_| ExpansionError::InvalidCursorIndex)?
    };
    Ok(cursors[..].get(index).cloned())
}
