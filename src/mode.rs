#![macro_use]

use std::mem::Discriminant;

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    config::Config,
    editor::{ReadLine, SearchText, EditorLoop, KeysIterator, StatusMessage},
    keymap::KeyMapCollection,
    picker::Picker,
    script::{ScriptContext, ScriptEngine},
    word_database::WordDatabase,
};

macro_rules! unwrap_or_return {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return,
        }
    };
}

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
pub mod picker;
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

    pub search: &'a mut SearchText,
    pub read_line: &'a mut ReadLine,
    pub picker: &'a mut Picker,

    pub status_message: &'a mut StatusMessage,

    pub keymaps: &'a mut KeyMapCollection,
    pub scripts: &'a mut ScriptEngine,
}

impl<'a> ModeContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.clients
            .get(self.target_client)
            .and_then(|c| c.current_buffer_view_handle)
    }

    pub fn script_context(&mut self) -> (&mut ScriptEngine, &mut ReadLine, ScriptContext) {
        let context = ScriptContext {
            target_client: self.target_client,
            clients: self.clients,
            editor_loop: EditorLoop::Continue,
            next_mode: Mode::default(),

            config: self.config,

            buffers: self.buffers,
            buffer_views: self.buffer_views,
            word_database: self.word_database,

            picker: self.picker,

            status_message: self.status_message,

            keymaps: self.keymaps,
        };

        (self.scripts, self.read_line, context)
    }
}

pub trait ModeState: Default {
    fn on_enter(&mut self, _context: &mut ModeContext) {}
    fn on_exit(&mut self, _context: &mut ModeContext) {}
    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation;
}

pub enum Mode {
    Normal(normal::State),
    Insert(insert::State),
    Search(search::State),
    Picker(picker::State),
    Goto(goto::State),
    Script(script::State),
}

impl Mode {
    pub fn discriminant(&self) -> Discriminant<Self> {
        std::mem::discriminant(self)
    }

    pub fn change_to(&mut self, ctx: &mut ModeContext, next: Mode) {
        match self {
            Mode::Normal(state) => state.on_exit(ctx),
            Mode::Insert(state) => state.on_exit(ctx),
            Mode::Search(state) => state.on_exit(ctx),
            Mode::Picker(state) => state.on_exit(ctx),
            Mode::Goto(state) => state.on_exit(ctx),
            Mode::Script(state) => state.on_exit(ctx),
        }

        *self = next;

        match self {
            Mode::Normal(state) => state.on_enter(ctx),
            Mode::Insert(state) => state.on_enter(ctx),
            Mode::Search(state) => state.on_enter(ctx),
            Mode::Picker(state) => state.on_enter(ctx),
            Mode::Goto(state) => state.on_enter(ctx),
            Mode::Script(state) => state.on_enter(ctx),
        }
    }

    pub fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match self {
            Mode::Normal(state) => state.on_event(ctx, keys),
            Mode::Insert(state) => state.on_event(ctx, keys),
            Mode::Search(state) => state.on_event(ctx, keys),
            Mode::Picker(state) => state.on_event(ctx, keys),
            Mode::Goto(state) => state.on_event(ctx, keys),
            Mode::Script(state) => state.on_event(ctx, keys),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal(Default::default())
    }
}
