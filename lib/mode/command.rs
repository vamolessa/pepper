use crate::{
    command::{CommandManager, CommandOperation},
    editor::{KeysIterator, ReadLinePoll},
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

                let op = CommandManager::eval_from_read_line(
                    ctx.editor,
                    ctx.platform,
                    ctx.clients,
                    Some(ctx.client_handle),
                );

                if ctx.editor.mode.kind() == ModeKind::Command {
                    Mode::change_to(ctx, ModeKind::default());
                }

                return match op {
                    Some(CommandOperation::Quit) => Some(ModeOperation::Quit),
                    Some(CommandOperation::QuitAll) => Some(ModeOperation::QuitAll),
                    None => None,
                };
            }
        }

        None
    }
}
