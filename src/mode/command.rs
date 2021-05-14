use std::fs;

use crate::{
    command::{CommandManager, CommandToken, CommandTokenIter, CommandTokenKind, CompletionSource},
    editor::KeysIterator,
    editor_utils::{hash_bytes, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    picker::Picker,
    platform::Key,
    word_database::WordIndicesIter,
};

enum ReadCommandState {
    NavigatingHistory(usize),
    TypingCommand,
}

pub struct State {
    read_state: ReadCommandState,
    completion_index: usize,
    completion_source: CompletionSource,
    completion_path_hash: Option<u64>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            read_state: ReadCommandState::TypingCommand,
            completion_index: 0,
            completion_source: CompletionSource::Custom(&[]),
            completion_path_hash: None,
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        let state = &mut ctx.editor.mode.command_state;
        state.read_state = ReadCommandState::NavigatingHistory(ctx.editor.commands.history_len());
        state.completion_index = 0;
        state.completion_source = CompletionSource::Custom(&[]);
        state.completion_path_hash = None;

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
        match ctx.editor.read_line.poll(
            ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
            &ctx.editor.registers,
        ) {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next(&ctx.editor.buffered_keys) {
                    Key::Ctrl('n') | Key::Ctrl('j') => match state.read_state {
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
                        ReadCommandState::TypingCommand => apply_completion(ctx, 1),
                    },
                    Key::Ctrl('p') | Key::Ctrl('k') => match state.read_state {
                        ReadCommandState::NavigatingHistory(ref mut i) => {
                            *i = i.saturating_sub(1);
                            let entry = ctx.editor.commands.history_entry(*i);
                            let input = ctx.editor.read_line.input_mut();
                            input.clear();
                            input.push_str(entry);
                        }
                        ReadCommandState::TypingCommand => apply_completion(ctx, -1),
                    },
                    _ => update_autocomplete_entries(ctx),
                }
            }
            ReadLinePoll::Canceled => Mode::change_to(ctx, ModeKind::default()),
            ReadLinePoll::Submitted => {
                let input = ctx.editor.read_line.input();
                ctx.editor.commands.add_to_history(input);

                let command = ctx.editor.string_pool.acquire_with(input);
                let operation = CommandManager::eval_and_then_output(
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
    if let Some((_, entry)) = ctx.editor.picker.current_entry(&ctx.editor.word_database) {
        let input = ctx.editor.read_line.input_mut();
        input.truncate(ctx.editor.mode.command_state.completion_index);
        input.push_str(entry);
    }
}

fn update_autocomplete_entries(ctx: &mut ModeContext) {
    let state = &mut ctx.editor.mode.command_state;

    let input = ctx.editor.read_line.input();
    let mut tokens = CommandTokenIter::new(input);

    let command_name = match tokens.next() {
        Some((_, token)) => token.as_str(input).trim_end_matches('!'),
        None => {
            ctx.editor.picker.clear();
            state.read_state =
                ReadCommandState::NavigatingHistory(ctx.editor.commands.history_len());
            state.completion_index = input.len();
            state.completion_source = CompletionSource::Custom(&[]);
            return;
        }
    };

    if let ReadCommandState::NavigatingHistory(_) = state.read_state {
        state.read_state = ReadCommandState::TypingCommand;
    }

    let mut is_flag_value = false;
    let mut arg_count = 0;
    let mut last_token = None;
    for token in tokens {
        match token.0 {
            CommandTokenKind::Text | CommandTokenKind::Register => {
                arg_count += !is_flag_value as usize
            }
            CommandTokenKind::Flag => is_flag_value = false,
            CommandTokenKind::Equals => is_flag_value = true,
            CommandTokenKind::Unterminated => arg_count += 1,
        }
        last_token = Some(token);
    }

    if input.ends_with(|c: char| c.is_ascii_whitespace()) {
        match last_token {
            Some((CommandTokenKind::Unterminated, _)) => (),
            None => {
                arg_count += 1;
                last_token = Some((CommandTokenKind::Text, CommandToken { from: 0, to: 0 }));
            }
            _ => arg_count += 1,
        }
    }

    let (completion_source, mut pattern) = match last_token {
        Some((CommandTokenKind::Text, token)) | Some((CommandTokenKind::Unterminated, token))
            if !is_flag_value =>
        {
            let mut completion_source = CompletionSource::Custom(&[]);
            if arg_count > 0 {
                for command in ctx.editor.commands.builtin_commands() {
                    if command.name == command_name || command.alias == command_name {
                        if let Some(&completion) = command.completions.get(arg_count - 1) {
                            completion_source = completion;
                        }
                        break;
                    }
                }
            }
            (completion_source, token.as_str(input))
        }
        None => (CompletionSource::Commands, command_name),
        _ => {
            ctx.editor.picker.clear();
            state.completion_index = input.len();
            state.completion_source = CompletionSource::Custom(&[]);
            return;
        }
    };

    state.completion_index = pattern.as_ptr() as usize - input.as_ptr() as usize;

    if state.completion_source != completion_source {
        state.completion_path_hash = None;
        ctx.editor.picker.clear();

        match completion_source {
            CompletionSource::Commands => {
                for command in ctx.editor.commands.builtin_commands() {
                    if !command.hidden {
                        ctx.editor.picker.add_custom_entry(command.name);
                    }
                }
                for command in ctx.editor.commands.macro_commands() {
                    if !command.hidden {
                        ctx.editor.picker.add_custom_entry(&command.name);
                    }
                }
                for command in ctx.editor.commands.request_commands() {
                    if !command.hidden {
                        ctx.editor.picker.add_custom_entry(&command.name);
                    }
                }
            }
            CompletionSource::Buffers => {
                for buffer in ctx.editor.buffers.iter() {
                    if let Some(path) = buffer.path.to_str() {
                        ctx.editor.picker.add_custom_entry(path);
                    }
                }
            }
            CompletionSource::Custom(completions) => {
                for completion in completions {
                    ctx.editor.picker.add_custom_entry(completion);
                }
            }
            _ => (),
        }
    }

    match completion_source {
        CompletionSource::Files => {
            fn set_files_in_path_as_entries(picker: &mut Picker, path: &str) {
                picker.clear();
                let path = if path.is_empty() { "." } else { path };
                let read_dir = match fs::read_dir(path) {
                    Ok(iter) => iter,
                    Err(_) => return,
                };
                for entry in read_dir {
                    let entry = match entry {
                        Ok(entry) => entry.file_name(),
                        Err(_) => return,
                    };
                    if let Some(entry) = entry.to_str() {
                        picker.add_custom_entry(entry);
                    }
                }
            }

            let (parent, file) = match pattern.rfind('/') {
                Some(i) => pattern.split_at(i + 1),
                None => ("", pattern),
            };

            let parent_hash = hash_bytes(parent.bytes());
            if state.completion_path_hash != Some(parent_hash) {
                set_files_in_path_as_entries(&mut ctx.editor.picker, parent);
                state.completion_path_hash = Some(parent_hash);
            }

            state.completion_index = file.as_ptr() as usize - input.as_ptr() as usize;
            pattern = file;
        }
        _ => (),
    }

    state.completion_source = completion_source;
    ctx.editor.picker.filter(WordIndicesIter::empty(), pattern);
}
