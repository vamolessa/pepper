#![macro_use]

use std::mem::Discriminant;

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    client_event::Key,
    config::Config,
    editor::{KeysIterator, StatusMessageKind},
    keymap::KeyMapCollection,
    picker::Picker,
    script::ScriptEngine,
    word_database::WordDatabase,
};

macro_rules! unwrap_or_none {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return ModeOperation::None,
        }
    };
}

mod goto;
mod insert;
mod normal;
mod script;
mod search;

pub enum ModeOperation {
    Pending,
    None,
    Quit,
    QuitAll,
    EnterMode(Mode),
}

pub struct ModeContext<'a> {
    pub target_client: TargetClient,
    pub clients: &'a mut ClientCollection,

    pub config: &'a mut Config,

    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub word_database: &'a mut WordDatabase,

    pub input: &'a mut String,
    pub picker: &'a mut Picker,

    pub status_message_kind: &'a mut StatusMessageKind,
    pub status_message: &'a mut String,

    pub keymaps: &'a mut KeyMapCollection,
    pub scripts: &'a mut ScriptEngine,
}

impl<'a> ModeContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.clients
            .get(self.target_client)
            .and_then(|c| c.current_buffer_view_handle)
    }
}

pub trait ModeState: Default {
    fn on_enter(&mut self, _context: &mut ModeContext) {}
    fn on_exit(&mut self, _context: &mut ModeContext) {}
    fn on_event(&mut self, context: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation;
}

pub enum Mode {
    Normal(normal::State),
    Insert(insert::State),
    Search(search::State),
    Goto(goto::State),
    Script(script::State),
}

impl Mode {
    pub fn discriminant(&self) -> Discriminant<Self> {
        std::mem::discriminant(self)
    }

    pub fn on_enter(&mut self, context: &mut ModeContext) {
        match self {
            Mode::Normal(state) => state.on_enter(context),
            Mode::Insert(state) => state.on_enter(context),
            Mode::Search(state) => state.on_enter(context),
            Mode::Goto(state) => state.on_enter(context),
            Mode::Script(state) => state.on_enter(context),
        }
    }

    pub fn on_exit(&mut self, context: &mut ModeContext) {
        match self {
            Mode::Normal(state) => state.on_exit(context),
            Mode::Insert(state) => state.on_exit(context),
            Mode::Search(state) => state.on_exit(context),
            Mode::Goto(state) => state.on_exit(context),
            Mode::Script(state) => state.on_exit(context),
        }
    }

    pub fn on_event(
        &mut self,
        context: &mut ModeContext,
        keys: &mut KeysIterator,
    ) -> ModeOperation {
        match self {
            Mode::Normal(state) => state.on_event(context, keys),
            Mode::Insert(state) => state.on_event(context, keys),
            Mode::Search(state) => state.on_event(context, keys),
            Mode::Goto(state) => state.on_event(context, keys),
            Mode::Script(state) => state.on_event(context, keys),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal(Default::default())
    }
}

pub enum InputPollResult {
    Pending,
    Submited,
    Canceled,
}

pub fn poll_input(ctx: &mut ModeContext, keys: &mut KeysIterator) -> InputPollResult {
    match keys.next() {
        Key::Esc => {
            ctx.input.clear();
            InputPollResult::Canceled
        }
        Key::Enter => InputPollResult::Submited,
        Key::Ctrl('u') => {
            ctx.input.clear();
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
            InputPollResult::Pending
        }
        Key::Ctrl('h') => {
            if let Some((last_char_index, _)) = ctx.input.char_indices().rev().next() {
                ctx.input.truncate(last_char_index);
            }
            InputPollResult::Pending
        }
        Key::Char(c) => {
            ctx.input.push(c);
            InputPollResult::Pending
        }
        _ => InputPollResult::Pending,
    }
}
