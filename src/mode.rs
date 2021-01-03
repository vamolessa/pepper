use std::{mem::Discriminant, path::Path};

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    config::Config,
    editor::{EditorLoop, KeysIterator, ReadLine, StatusMessage},
    editor_event::{EditorEvent, EditorEventQueue, EditorEventsIter},
    keymap::KeyMapCollection,
    lsp::LspClientCollection,
    picker::Picker,
    register::{RegisterCollection, RegisterKey},
    script::{ScriptContext, ScriptEngine},
    task::TaskManager,
    word_database::WordDatabase,
};

mod insert;
mod normal;
pub mod picker;
pub mod read_line;
mod script;

pub enum ModeOperation {
    Pending,
    None,
    Quit,
    QuitAll,
    EnterMode(Mode),
    ExecuteMacro(RegisterKey),
}

pub struct ModeContext<'a> {
    pub target_client: TargetClient,
    pub clients: &'a mut ClientCollection,

    pub current_directory: &'a Path,
    pub config: &'a mut Config,

    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub word_database: &'a mut WordDatabase,

    pub recording_macro: &'a mut Option<RegisterKey>,
    pub registers: &'a mut RegisterCollection,
    pub read_line: &'a mut ReadLine,
    pub picker: &'a mut Picker,

    pub status_message: &'a mut StatusMessage,

    pub editor_events: &'a mut EditorEventQueue,
    pub keymaps: &'a mut KeyMapCollection,
    pub scripts: &'a mut ScriptEngine,
    pub tasks: &'a mut TaskManager,
    pub lsp: &'a mut LspClientCollection,
}

impl<'a> ModeContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.clients
            .get(self.target_client)
            .and_then(|c| c.current_buffer_view_handle())
    }

    pub fn set_current_buffer_view_handle(&mut self, handle: Option<BufferViewHandle>) {
        if let Some(client) = self.clients.get_mut(self.target_client) {
            client.set_current_buffer_view_handle(handle);

            if let Some(handle) = handle
                .and_then(|h| self.buffer_views.get(h))
                .map(|v| v.buffer_handle)
            {
                self.editor_events
                    .enqueue(EditorEvent::BufferOpen { handle });
            }
        }
    }

    pub fn into_script_context(&mut self) -> (&mut ScriptEngine, ScriptContext) {
        let ctx = ScriptContext {
            target_client: self.target_client,
            clients: self.clients,
            editor_loop: EditorLoop::Continue,
            next_mode: Mode::default(),
            edited_buffers: false,

            current_directory: self.current_directory,
            config: self.config,

            buffers: self.buffers,
            buffer_views: self.buffer_views,
            word_database: self.word_database,

            registers: self.registers,
            read_line: self.read_line,
            picker: self.picker,

            status_message: self.status_message,

            editor_events: self.editor_events,
            keymaps: self.keymaps,
            tasks: self.tasks,
            lsp: self.lsp,
        };

        (self.scripts, ctx)
    }
}

pub trait ModeState {
    fn on_enter(&mut self, _ctx: &mut ModeContext) {}
    fn on_exit(&mut self, _ctx: &mut ModeContext) {}
    fn on_client_keys(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation;
    fn on_editor_events(&mut self, _ctx: &mut ModeContext, _events: EditorEventsIter) {}
}

pub enum Mode {
    Normal(normal::State),
    Insert(insert::State),
    ReadLine(read_line::State),
    Picker(picker::State),
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
            Mode::ReadLine(state) => state.on_exit(ctx),
            Mode::Picker(state) => state.on_exit(ctx),
            Mode::Script(state) => state.on_exit(ctx),
        }

        *self = next;

        match self {
            Mode::Normal(state) => state.on_enter(ctx),
            Mode::Insert(state) => state.on_enter(ctx),
            Mode::ReadLine(state) => state.on_enter(ctx),
            Mode::Picker(state) => state.on_enter(ctx),
            Mode::Script(state) => state.on_enter(ctx),
        }
    }

    pub fn on_client_keys(
        &mut self,
        ctx: &mut ModeContext,
        keys: &mut KeysIterator,
    ) -> ModeOperation {
        match self {
            Mode::Normal(state) => state.on_client_keys(ctx, keys),
            Mode::Insert(state) => state.on_client_keys(ctx, keys),
            Mode::ReadLine(state) => state.on_client_keys(ctx, keys),
            Mode::Picker(state) => state.on_client_keys(ctx, keys),
            Mode::Script(state) => state.on_client_keys(ctx, keys),
        }
    }

    pub fn on_editor_events(&mut self, ctx: &mut ModeContext, events: EditorEventsIter) {
        match self {
            Mode::Normal(state) => state.on_editor_events(ctx, events),
            Mode::Insert(state) => state.on_editor_events(ctx, events),
            Mode::ReadLine(state) => state.on_editor_events(ctx, events),
            Mode::Picker(state) => state.on_editor_events(ctx, events),
            Mode::Script(state) => state.on_editor_events(ctx, events),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal(Default::default())
    }
}
