use crate::{
    command::{
        CommandError, CommandManager, CommandOperation, CommandSourceIter, CommandTokenIter,
        CommandTokenKind,
    },
    editor::KeysIterator,
    editor_utils::{MessageKind, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    platform::Key,
    word_database::WordIndicesIter,
};

enum CompletionState {
    None,
    CommandName,
}

enum ReadCommandState {
    NavigatingHistory(usize),
    TypingCommand(CompletionState),
}

pub struct State {
    picker_state: ReadCommandState,
    completion_index: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            picker_state: ReadCommandState::TypingCommand(CompletionState::None),
            completion_index: 0,
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.mode.command_state.picker_state =
            ReadCommandState::NavigatingHistory(ctx.editor.commands.history_len());
        ctx.editor.read_line.set_prompt(":");
        ctx.editor.read_line.input_mut().clear();
        ctx.editor.picker.clear();
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.read_line.input_mut().clear();
        ctx.editor.picker.clear();
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
                        ReadCommandState::NavigatingHistory(ref mut i) => {
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
                        ReadCommandState::TypingCommand(_) => apply_completion(ctx, 1),
                    },
                    Key::Ctrl('p') | Key::Ctrl('k') => match state.picker_state {
                        ReadCommandState::NavigatingHistory(ref mut i) => {
                            *i = i.saturating_sub(1);
                            let entry = ctx.editor.commands.history_entry(*i);
                            let input = ctx.editor.read_line.input_mut();
                            input.clear();
                            input.push_str(entry);
                        }
                        ReadCommandState::TypingCommand(_) => apply_completion(ctx, -1),
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

                let command = ctx.editor.string_pool.acquire_with(input);
                let operation = CommandManager::eval_commands_then_output(
                    ctx.editor,
                    ctx.platform,
                    ctx.clients,
                    Some(ctx.client_handle),
                    &command,
                    None,
                )
                .map(From::from);
                ctx.editor.string_pool.release(command);

                if ctx.editor.mode.kind() == ModeKind::Command {
                    Mode::change_to(ctx, ModeKind::default());
                }

                return operation;
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

    let input = ctx.editor.read_line.input();
    let trimmed_input = input.trim_start();
    let mut tokens = CommandTokenIter(trimmed_input);

    if tokens.next().is_none() {
        ctx.editor.picker.clear();
        state.picker_state = ReadCommandState::NavigatingHistory(ctx.editor.commands.history_len());
        state.completion_index = input.len();
        return;
    }

    let completion_state = match &mut state.picker_state {
        ReadCommandState::NavigatingHistory(_) => {
            state.picker_state = ReadCommandState::TypingCommand(CompletionState::None);
            match &mut state.picker_state {
                ReadCommandState::NavigatingHistory(_) => unreachable!(),
                ReadCommandState::TypingCommand(state) => state,
            }
        }
        ReadCommandState::TypingCommand(state) => state,
    };

    let mut is_flag_value = false;
    let mut value_arg_count = 0;
    let mut last_token = None;
    for token in tokens {
        match token.0 {
            CommandTokenKind::Text => value_arg_count += is_flag_value as usize,
            CommandTokenKind::Flag => is_flag_value = false,
            CommandTokenKind::Equals => is_flag_value = true,
            CommandTokenKind::Unterminated => (),
        }
        last_token = Some(token);
    }

    if trimmed_input.ends_with(|c: char| c.is_ascii_whitespace()) {
        match last_token {
            Some((CommandTokenKind::Unterminated, _)) => (),
            None => {
                value_arg_count += 1;
                last_token = Some((CommandTokenKind::Text, ""));
            }
            _ => value_arg_count += 1,
        }
    }

    match last_token {
        Some((CommandTokenKind::Text, token)) | Some((CommandTokenKind::Unterminated, token))
            if !is_flag_value =>
        {
            // TODO: complete value
        }
        Some((CommandTokenKind::Flag, token)) => {
            // TODO: complete flag
        }
        None => {
            if !matches!(completion_state, CompletionState::CommandName) {
                *completion_state = CompletionState::CommandName;
                ctx.editor.picker.clear();
                state.completion_index = input.len() - trimmed_input.len();
            }

            let command_name = trimmed_input.trim_end_matches('!');
            ctx.editor.picker.filter(
                WordIndicesIter::empty(),
                ctx.editor.commands.command_sources(),
                command_name,
            );
        }
        _ => {
            *completion_state = CompletionState::None;
            ctx.editor.picker.clear();
            state.completion_index = input.len();
        }
    }
}
