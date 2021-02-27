use crate::{
    command::{CommandError, CommandManager, CommandOperation},
    editor::KeysIterator,
    editor_utils::{MessageKind, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    platform::Key,
};

pub enum State {
    NavigatingHistory(usize),
    TypingCommand,
}
impl Default for State {
    fn default() -> Self {
        Self::TypingCommand
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.mode.command_state = State::NavigatingHistory(ctx.editor.commands.history_len());
        ctx.editor.read_line.set_prompt(":");
        ctx.editor.read_line.set_input("");
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.read_line.set_input("");
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        let state = &mut ctx.editor.mode.command_state;
        match ctx
            .editor
            .read_line
            .poll(ctx.platform, &ctx.editor.buffered_keys, keys)
        {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next(&ctx.editor.buffered_keys) {
                    Key::Ctrl('n') | Key::Ctrl('j') => match state {
                        State::NavigatingHistory(i) => {
                            *i = ctx
                                .editor
                                .commands
                                .history_len()
                                .saturating_sub(1)
                                .min(*i + 1);
                            let entry = ctx.editor.commands.history_entry(*i);
                            ctx.editor.read_line.set_input(entry);
                        }
                        State::TypingCommand => autocomplete(ctx),
                    },
                    Key::Ctrl('p') | Key::Ctrl('k') => match state {
                        State::NavigatingHistory(i) => {
                            *i = i.saturating_sub(1);
                            let entry = ctx.editor.commands.history_entry(*i);
                            ctx.editor.read_line.set_input(entry);
                        }
                        State::TypingCommand => autocomplete(ctx),
                    },
                    _ => {
                        *state = if ctx.editor.read_line.input().is_empty() {
                            State::NavigatingHistory(ctx.editor.commands.history_len())
                        } else {
                            State::TypingCommand
                        };
                    }
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
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!(
                            "command is too long. max is {} bytes. got {}",
                            command_buf.len(),
                            input.len()
                        ));
                    return None;
                }
                command_buf[..input.len()].copy_from_slice(input.as_bytes());
                let command = unsafe { std::str::from_utf8_unchecked(&command_buf[..input.len()]) };

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
                        let buffers = &ctx.editor.buffers;
                        ctx.editor
                            .status_bar
                            .write(MessageKind::Error)
                            .fmt(format_args!("{}", error.display(command, buffers)));
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

fn autocomplete(ctx: &mut ModeContext) {
    let input = ctx.editor.read_line.input();
    //
}
