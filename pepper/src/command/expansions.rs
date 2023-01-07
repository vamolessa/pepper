use std::{env, fmt, process::Stdio};

use crate::{
    buffer::BufferHandle,
    command::{CommandManager, ExpansionError},
    editor_utils::{parse_process_command, to_absolute_path_string, LogKind, RegisterKey},
};

pub fn register_expansions(commands: &mut CommandManager) {
    use fmt::Write;

    let mut r = |name, expansion_fn| {
        commands.register_expansion(None, name, expansion_fn);
    };

    r("client-id", |_, io| {
        io.assert_empty_args()?;
        if let Some(client_handle) = io.client_handle {
            let _ = write!(io.output, "{}", client_handle.0);
        }
        Ok(())
    });

    r("buffer-id", |ctx, io| {
        io.assert_empty_args()?;
        if let Some(buffer) = io.current_buffer(ctx) {
            let _ = write!(io.output, "{}", buffer.handle().0);
        }
        Ok(())
    });

    r("buffer-path", |ctx, io| {
        let buffer = if io.args.is_empty() {
            io.current_buffer(ctx)
        } else {
            let id = io
                .args
                .parse()
                .map_err(|_| ExpansionError::InvalidBufferId)?;
            ctx.editor.buffers.try_get(BufferHandle(id))
        };
        if let Some(path) = buffer.and_then(|b| b.path.to_str()) {
            io.output.push_str(path);
        }
        Ok(())
    });

    r("buffer-absolute-path", |ctx, io| {
        let buffer = if io.args.is_empty() {
            io.current_buffer(ctx)
        } else {
            let id = io
                .args
                .parse()
                .map_err(|_| ExpansionError::InvalidBufferId)?;
            ctx.editor.buffers.try_get(BufferHandle(id))
        };
        let current_directory = ctx.editor.current_directory.to_str();
        let path = buffer.and_then(|b| b.path.to_str());
        if let (Some(current_directory), Some(path)) = (current_directory, path) {
            to_absolute_path_string(current_directory, path, io.output);
        }
        Ok(())
    });

    r("buffer-content", |ctx, io| {
        let buffer = if io.args.is_empty() {
            io.current_buffer(ctx)
        } else {
            let id = io
                .args
                .parse()
                .map_err(|_| ExpansionError::InvalidBufferId)?;
            let handle = BufferHandle(id);
            ctx.editor.buffers.try_get(handle)
        };
        if let Some(buffer) = buffer {
            for line in buffer.content().lines() {
                io.output.push_str(line.as_str());
                io.output.push('\n');
            }
        }
        Ok(())
    });

    r("cursor-anchor", |ctx, io| {
        if let Some(cursor) = io.cursor(ctx)? {
            let _ = write!(io.output, "{}", cursor.anchor);
        }
        Ok(())
    });

    r("cursor-anchor-column", |ctx, io| {
        if let Some(cursor) = io.cursor(ctx)? {
            let _ = write!(io.output, "{}", cursor.anchor.column_byte_index + 1);
        }
        Ok(())
    });

    r("cursor-anchor-line", |ctx, io| {
        if let Some(cursor) = io.cursor(ctx)? {
            let _ = write!(io.output, "{}", cursor.anchor.line_index + 1);
        }
        Ok(())
    });

    r("cursor-position", |ctx, io| {
        if let Some(cursor) = io.cursor(ctx)? {
            let _ = write!(io.output, "{}", cursor.position);
        }
        Ok(())
    });

    r("cursor-position-column", |ctx, io| {
        if let Some(cursor) = io.cursor(ctx)? {
            let _ = write!(io.output, "{}", cursor.position.column_byte_index + 1);
        }
        Ok(())
    });

    r("cursor-position-line", |ctx, io| {
        if let Some(cursor) = io.cursor(ctx)? {
            let _ = write!(io.output, "{}", cursor.position.line_index + 1);
        }
        Ok(())
    });

    r("cursor-selection", |ctx, io| {
        let buffer = io.current_buffer(ctx);
        let cursor = io.cursor(ctx)?;
        if let (Some(buffer), Some(cursor)) = (buffer, cursor) {
            let range = cursor.to_range();
            for text in buffer.content().text_range(range) {
                io.output.push_str(text);
            }
        }
        Ok(())
    });

    r("picker-entry", |ctx, io| {
        io.assert_empty_args()?;
        let entry = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
            Some(entry) => entry.1,
            None => "",
        };
        io.output.push_str(entry);
        Ok(())
    });

    r("register", |ctx, io| {
        let key = RegisterKey::from_str(io.args).ok_or(ExpansionError::InvalidRegisterKey)?;
        io.output.push_str(ctx.editor.registers.get(key));
        Ok(())
    });

    r("session-name", |ctx, io| {
        io.assert_empty_args()?;
        io.output.push_str(&ctx.editor.session_name);
        Ok(())
    });

    r("platform", |_, io| {
        io.assert_empty_args()?;
        let current_platform = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(any(
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly",
        )) {
            "bsd"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "unknown"
        };
        io.output.push_str(current_platform);
        Ok(())
    });

    r("cwd", |_, io| {
        io.assert_empty_args()?;
        if let Ok(current_dir) = env::current_dir() {
            if let Ok(current_dir) = current_dir.into_os_string().into_string() {
                io.output.push_str(&current_dir);
            }
        }
        Ok(())
    });

    r("pid", |_, io| {
        io.assert_empty_args()?;
        let _ = write!(io.output, "{}", std::process::id());
        Ok(())
    });

    r("env", |_, io| {
        if let Ok(env_var) = env::var(io.args) {
            io.output.push_str(&env_var);
        }
        Ok(())
    });

    r("output", |ctx, io| {
        let mut command =
            parse_process_command(io.args).ok_or(ExpansionError::InvalidProcessCommand)?;
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());
        if let Ok(output) = command.output() {
            if let Ok(output) = String::from_utf8(output.stdout) {
                io.output.push_str(&output);
            }
        }

        ctx.editor
            .logger
            .write(LogKind::Diagnostic)
            .fmt(format_args!("@output({})", io.args));

        Ok(())
    });
}
