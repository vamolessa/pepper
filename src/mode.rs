use std::mem::Discriminant;

use serde_derive::{Deserialize, Serialize};

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client_event::Key,
    config::Config,
    connection::TargetClient,
    editor::KeysIterator,
    editor_operation::{EditorOperation, EditorOperationSerializer},
    keymap::KeyMapCollection,
    script::ScriptEngine,
};

mod insert;
mod normal;
mod script;
mod search;
mod select;

pub enum ModeOperation {
    Pending,
    None,
    Quit,
    QuitAll,
    EnterMode(Mode),
}

pub struct ModeContext<'a> {
    pub input: &'a mut String,
    pub target_client: TargetClient,
    pub operations: &'a mut EditorOperationSerializer,
    pub scripts: &'a mut ScriptEngine,

    pub config: &'a Config,
    pub keymaps: &'a mut KeyMapCollection,
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,

    pub local_client_current_buffer_view_handle: &'a mut Option<BufferViewHandle>,
    pub remote_client_current_buffer_view_handles: &'a mut [Option<BufferViewHandle>],
}

impl<'a> ModeContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        match self.target_client {
            TargetClient::All => unreachable!(),
            TargetClient::Local => self.local_client_current_buffer_view_handle.clone(),
            TargetClient::Remote(handle) => {
                self.remote_client_current_buffer_view_handles[handle.into_index()].clone()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FromMode {
    Normal,
    Select,
}

impl FromMode {
    pub fn as_mode(&self) -> Mode {
        match self {
            FromMode::Normal => Mode::Normal,
            FromMode::Select => Mode::Select,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Mode {
    Normal,
    Select,
    Insert,
    Search(FromMode),
    Script(FromMode),
}

impl Mode {
    pub fn discriminant(&self) -> Discriminant<Self> {
        std::mem::discriminant(self)
    }

    pub fn on_enter(&mut self, context: &mut ModeContext) {
        match self {
            Mode::Normal => normal::on_enter(context),
            Mode::Select => select::on_enter(context),
            Mode::Insert => insert::on_enter(context),
            Mode::Search(_) => search::on_enter(context),
            Mode::Script(_) => script::on_enter(context),
        }
    }

    pub fn on_event(
        &mut self,
        context: &mut ModeContext,
        keys: &mut KeysIterator,
    ) -> ModeOperation {
        match self {
            Mode::Normal => normal::on_event(context, keys),
            Mode::Select => select::on_event(context, keys),
            Mode::Insert => insert::on_event(context, keys),
            Mode::Search(from_mode) => search::on_event(context, keys, from_mode),
            Mode::Script(from_mode) => script::on_event(context, keys, from_mode),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

pub enum InputPollResult {
    Pending,
    Submited,
    Canceled,
}

pub fn poll_input(ctx: &mut ModeContext, keys: &mut KeysIterator) -> InputPollResult {
    match keys.next() {
        Key::Esc | Key::Ctrl('c') => {
            ctx.input.clear();
            ctx.operations
                .serialize(TargetClient::All, &EditorOperation::InputKeep(0));
            InputPollResult::Canceled
        }
        Key::Ctrl('m') => InputPollResult::Submited,
        Key::Ctrl('u') => {
            ctx.input.clear();
            ctx.operations
                .serialize(TargetClient::All, &EditorOperation::InputKeep(0));
            InputPollResult::Pending
        }
        Key::Ctrl('w') => {
            let mut found_space = false;
            let mut last_index = 0;
            for (i, c) in ctx.input.char_indices().rev() {
                if found_space {
                    if c != ' ' {
                        break;
                    }
                } else if c == ' ' {
                    found_space = true;
                }
                last_index = i;
            }

            ctx.input.truncate(last_index);
            ctx.operations
                .serialize(TargetClient::All, &EditorOperation::InputKeep(last_index));
            InputPollResult::Pending
        }
        Key::Ctrl('h') => {
            if let Some((last_char_index, _)) = ctx.input.char_indices().rev().next() {
                ctx.input.truncate(last_char_index);
                ctx.operations.serialize(
                    TargetClient::All,
                    &EditorOperation::InputKeep(last_char_index),
                );
            }
            InputPollResult::Pending
        }
        Key::Char(c) => {
            ctx.input.push(c);
            ctx.operations
                .serialize(TargetClient::All, &EditorOperation::InputAppend(c));
            InputPollResult::Pending
        }
        _ => InputPollResult::Pending,
    }
}
