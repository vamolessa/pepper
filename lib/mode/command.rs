use std::{fs, path::Path};

use crate::{
    command::{
        CommandManager, CommandSourceIter, CommandTokenIter, CommandTokenKind, CompletionSource,
    },
    editor::KeysIterator,
    editor_utils::ReadLinePoll,
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    picker::Picker,
    platform::Key,
    word_database::WordIndicesIter,
};

#[derive(PartialEq, Eq)]
enum CompletionState {
    None,
    CommandName,
    Argument(usize),
}

enum ReadCommandState {
    NavigatingHistory(usize),
    TypingCommand(CompletionState),
}

pub struct State {
    picker_state: ReadCommandState,
    completion_index: usize,
    completion_source: CompletionSource,
    completion_path: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            picker_state: ReadCommandState::TypingCommand(CompletionState::None),
            completion_index: 0,
            completion_source: CompletionSource::Custom(&[]),
            completion_path: String::new(),
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

    let command_name = match tokens.next() {
        Some((_, token)) => token.trim_end_matches('!'),
        None => {
            ctx.editor.picker.clear();
            state.picker_state =
                ReadCommandState::NavigatingHistory(ctx.editor.commands.history_len());
            state.completion_index = input.len();
            return;
        }
    };

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
    let mut arg_count = 0;
    let mut last_token = None;
    for token in tokens {
        match token.0 {
            CommandTokenKind::Text => arg_count += !is_flag_value as usize,
            CommandTokenKind::Flag => is_flag_value = false,
            CommandTokenKind::Equals => is_flag_value = true,
            CommandTokenKind::Unterminated => arg_count += 1,
        }
        last_token = Some(token);
    }
    drop(is_flag_value);

    if trimmed_input.ends_with(|c: char| c.is_ascii_whitespace()) {
        match last_token {
            Some((CommandTokenKind::Unterminated, _)) => (),
            None => {
                arg_count += 1;
                last_token = Some((CommandTokenKind::Text, ""));
            }
            _ => arg_count += 1,
        }
    }

    match last_token {
        Some((CommandTokenKind::Text, token)) | Some((CommandTokenKind::Unterminated, token))
            if !is_flag_value =>
        {
            fn add_files_in_path(
                picker: &mut Picker,
                completion_path: &mut String,
                current_path: &str,
            ) {
                if current_path == completion_path {
                    return;
                }

                picker.clear();
                completion_path.clear();
                completion_path.push_str(current_path);

                let read_dir = match fs::read_dir(completion_path) {
                    Ok(iter) => iter,
                    Err(_) => return,
                };
                for entry in read_dir {
                    let entry = match entry {
                        Ok(entry) => entry.file_name(),
                        Err(_) => return,
                    };
                    if let Some(entry) = entry.to_str() {
                        picker.add_custom_entry(entry, "");
                    }
                }
            }

            let arg_index = arg_count - 1;
            if *completion_state != CompletionState::Argument(arg_index) {
                *completion_state = CompletionState::Argument(arg_index);
                ctx.editor.picker.clear();
                state.completion_index = match last_token {
                    Some((CommandTokenKind::Text, _)) => input.trim_end().len() - token.len(),
                    Some((CommandTokenKind::Unterminated, _)) => input.len() - token.len(),
                    _ => unreachable!(),
                };
                state.completion_source = CompletionSource::Custom(&[]);
                state.completion_path.clear();
                if token.is_empty() {
                    state.completion_path.push('.');
                }

                for command in ctx.editor.commands.builtin_commands() {
                    if (command.name == command_name || command.alias == command_name)
                        && arg_index < command.completions.len()
                    {
                        state.completion_source = command.completions[arg_index];
                        break;
                    }
                }

                match state.completion_source {
                    CompletionSource::Commands => (),
                    CompletionSource::Buffers => {
                        for buffer in ctx.editor.buffers.iter() {
                            if let Some(path) = buffer.path().and_then(Path::to_str) {
                                let changed = if buffer.needs_save() { "changed" } else { "" };
                                ctx.editor.picker.add_custom_entry(path, changed);
                            }
                        }
                    }
                    CompletionSource::Files => (),
                    CompletionSource::Custom(completions) => {
                        for completion in completions {
                            ctx.editor.picker.add_custom_entry(completion, "");
                        }
                    }
                }
            }

            let (command_sources, pattern) = match state.completion_source {
                CompletionSource::Commands => (ctx.editor.commands.command_sources(), token),
                CompletionSource::Files => {
                    let (parent, file) = match token.rfind('/') {
                        Some(i) => (&token[..i], &token[(i + 1)..]),
                        None => ("", token),
                    };

                    add_files_in_path(&mut ctx.editor.picker, &mut state.completion_path, parent);
                    (CommandSourceIter::empty(), file)
                }
                _ => (CommandSourceIter::empty(), token),
            };

            ctx.editor
                .picker
                .filter(WordIndicesIter::empty(), command_sources, pattern);
        }
        Some((CommandTokenKind::Flag, _)) => {
            *completion_state = CompletionState::None;
            ctx.editor.picker.clear();
            state.completion_index = input.len();
        }
        None => {
            if *completion_state != CompletionState::CommandName {
                *completion_state = CompletionState::CommandName;
                ctx.editor.picker.clear();
                state.completion_index = input.len() - trimmed_input.len();
            }

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
