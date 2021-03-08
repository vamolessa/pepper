use crate::{
    command::{CommandError, CommandManager, CommandOperation, CommandTokenIter, CommandTokenKind},
    editor::KeysIterator,
    editor_utils::{MessageKind, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    platform::Key,
};

enum CompletionState {
    Dirty,
    CommandName,
}

enum PickerState {
    NavigatingHistory(usize),
    TypingCommand(CompletionState),
}

pub struct State {
    picker_state: PickerState,
    completion_index: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            picker_state: PickerState::TypingCommand(CompletionState::Dirty),
            completion_index: 0,
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.mode.command_state.picker_state =
            PickerState::NavigatingHistory(ctx.editor.commands.history_len());
        ctx.editor.read_line.set_prompt(":");
        ctx.editor.read_line.input_mut().clear();
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.read_line.input_mut().clear();
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
                    Key::Ctrl('n') | Key::Ctrl('j') => match state.picker_state {
                        PickerState::NavigatingHistory(ref mut i) => {
                            *i = ctx
                                .editor
                                .commands
                                .history_len()
                                .saturating_sub(1)
                                .min(*i + 1);
                            let entry = ctx.editor.commands.history_entry(*i);
                            let input = ctx.editor.read_line.input_mut();
                            input.clear();
                            input.push_str(entry);
                        }
                        PickerState::TypingCommand(_) => apply_completion(ctx, 1),
                    },
                    Key::Ctrl('p') | Key::Ctrl('k') => match state.picker_state {
                        PickerState::NavigatingHistory(ref mut i) => {
                            *i = i.saturating_sub(1);
                            let entry = ctx.editor.commands.history_entry(*i);
                            let input = ctx.editor.read_line.input_mut();
                            input.clear();
                            input.push_str(entry);
                        }
                        PickerState::TypingCommand(_) => apply_completion(ctx, -1),
                    },
                    _ => update_autocomplete_entries(ctx),
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
                    Ok(None) => None,
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

fn apply_completion(ctx: &mut ModeContext, cursor_movement: isize) {
    ctx.editor.picker.move_cursor(cursor_movement);
    if let Some(entry) = ctx
        .editor
        .picker
        .current_entry(&ctx.editor.word_database, &ctx.editor.commands)
    {
        let input = ctx.editor.read_line.input_mut();
        input.truncate(ctx.editor.mode.command_state.completion_index);
        input.push_str(entry.name);
    }
}

fn update_autocomplete_entries(ctx: &mut ModeContext) {
    let state = &mut ctx.editor.mode.command_state;
    let completion_state = match &mut state.picker_state {
        PickerState::NavigatingHistory(_) => return,
        PickerState::TypingCommand(state) => state,
    };

    let input = ctx.editor.read_line.input();
    let mut tokens = CommandTokenIter { rest: input };

    let first_token = match tokens.next() {
        Some((_, token)) => token,
        None => {
            ctx.editor.picker.clear();
            state.picker_state = PickerState::NavigatingHistory(ctx.editor.commands.history_len());
            state.completion_index = 0;
            return;
        }
    };

    let mut value_arg_count = 0;
    match tokens.last() {
        Some((kind, token)) => {
            // TODO: complete parameter
        }
        None => {
            // TODO: complete command name

            state.completion_index = first_token.as_ptr() as usize - input.as_ptr() as usize;
            if !matches!(completion_state, CompletionState::CommandName) {
                *completion_state = CompletionState::CommandName;
            }
        }
    }
}
