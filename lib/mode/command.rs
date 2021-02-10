use crate::platform::Key;

use crate::{
    client::{ClientManager, ClientHandle},
    command::{CommandManager, CommandOperation},
    editor::{Editor, KeysIterator, ReadLinePoll},
    mode::{Mode, ModeKind, ModeOperation, ModeState},
};

#[derive(Default)]
pub struct State {
    history_index: usize,
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor, _: &mut ClientManager, _: ClientHandle) {
        editor.mode.command_state.history_index = editor.commands.history_len();
        editor.read_line.set_prompt(":");
        editor.read_line.set_input("");
    }

    fn on_exit(editor: &mut Editor, _: &mut ClientManager, _: ClientHandle) {
        editor.read_line.set_input("");
    }

    fn on_client_keys(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<ModeOperation> {
        let this = &mut editor.mode.command_state;
        match editor.read_line.poll(&editor.buffered_keys, keys) {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next(&editor.buffered_keys) {
                    Key::Ctrl('n') | Key::Ctrl('j') => {
                        if editor.picker.len() == 0 {
                            this.history_index = editor
                                .commands
                                .history_len()
                                .saturating_sub(1)
                                .min(this.history_index + 1);
                            let entry = editor.commands.history_entry(this.history_index);
                            editor.read_line.set_input(entry);
                        } else {
                            // TODO: autocomplete
                        }
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        if editor.picker.len() == 0 {
                            this.history_index = this.history_index.saturating_sub(1);
                            let entry = editor.commands.history_entry(this.history_index);
                            editor.read_line.set_input(entry);
                        } else {
                            // TODO: autocomplete
                        }
                    }
                    _ => (),
                }
            }
            ReadLinePoll::Canceled => {
                Mode::change_to(editor, clients, client_handle, ModeKind::default())
            }
            ReadLinePoll::Submitted => {
                let input = editor.read_line.input();
                if !input.starts_with(|c: char| c.is_ascii_whitespace()) {
                    editor.commands.add_to_history(input);
                }

                let op = CommandManager::eval_from_read_line(editor, clients, Some(client_handle));

                if editor.mode.kind() == ModeKind::Command {
                    Mode::change_to(editor, clients, client_handle, ModeKind::default());
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
