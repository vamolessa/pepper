use crate::{
    command::{CommandError, CommandManager, CommandOperation},
    editor::KeysIterator,
    editor_utils::{EditorOutputKind, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    platform::Key,
};

#[derive(Default)]
pub struct State {
    history_index: usize,
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.mode.command_state.history_index = ctx.editor.commands.history_len();
        ctx.editor.read_line.set_prompt(":");
        ctx.editor.read_line.set_input("");
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.read_line.set_input("");
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        let this = &mut ctx.editor.mode.command_state;
        match ctx
            .editor
            .read_line
            .poll(ctx.platform, &ctx.editor.buffered_keys, keys)
        {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next(&ctx.editor.buffered_keys) {
                    Key::Ctrl('n') | Key::Ctrl('j') => {
                        if ctx.editor.picker.len() == 0 {
                            this.history_index = ctx
                                .editor
                                .commands
                                .history_len()
                                .saturating_sub(1)
                                .min(this.history_index + 1);
                            let entry = ctx.editor.commands.history_entry(this.history_index);
                            ctx.editor.read_line.set_input(entry);
                        } else {
                            // TODO: autocomplete
                        }
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        if ctx.editor.picker.len() == 0 {
                            this.history_index = this.history_index.saturating_sub(1);
                            let entry = ctx.editor.commands.history_entry(this.history_index);
                            ctx.editor.read_line.set_input(entry);
                        } else {
                            // TODO: autocomplete
                        }
                    }
                    _ => (),
                }
            }
            ReadLinePoll::Canceled => Mode::change_to(ctx, ModeKind::default()),
            ReadLinePoll::Submitted => {
                let input = ctx.editor.read_line.input();
                if !input.starts_with(|c: char| c.is_ascii_whitespace()) {
                    ctx.editor.commands.add_to_history(input);
                }

                let mut command_buf = [0; 256];
                if input.len() > command_buf.len() {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!(
                            "command is too long. max is {} bytes. got {}",
                            command_buf.len(),
                            input.len()
                        ));
                    return None;
                }
                command_buf[..input.len()].copy_from_slice(input.as_bytes());
                let command = unsafe { std::str::from_utf8_unchecked(&command_buf[..input.len()]) };

                // TODO: prevent allocation here
                let mut output = String::new();
                let op = CommandManager::eval_command(
                    ctx.editor,
                    ctx.platform,
                    ctx.clients,
                    Some(ctx.client_handle),
                    command,
                    &mut output,
                );
                let op = match op {
                    Ok(None) | Err(CommandError::Aborted) => None,
                    Ok(Some(CommandOperation::Quit)) => Some(ModeOperation::Quit),
                    Ok(Some(CommandOperation::QuitAll)) => Some(ModeOperation::QuitAll),
                    Err(error) => {
                        ctx.editor
                            .output
                            .write(EditorOutputKind::Error)
                            .fmt(format_args!("{}", error.display(command)));
                        None
                    }
                };

                if ctx.editor.mode.kind() == ModeKind::Command {
                    Mode::change_to(ctx, ModeKind::default());
                }

                return op;
            }
        }

        None
    }
}
