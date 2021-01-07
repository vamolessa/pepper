use std::path::Path;

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    config::Config,
    editor::{EditorLoop, KeysIterator, ReadLine, StatusMessage, Editor},
    editor_event::{EditorEvent, EditorEventQueue, EditorEventsIter},
    keymap::KeyMapCollection,
    lsp::LspClientCollection,
    picker::Picker,
    register::{RegisterCollection, RegisterKey},
    script::{ScriptContext, ScriptEngine},
    script_bindings::ScriptCallbacks,
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
    EnterMode(ModeKind),
    ExecuteMacro(RegisterKey),
}

pub struct ModeContext<'a> {
    pub target_client: TargetClient,
    pub clients: &'a mut ClientCollection,

    pub current_directory: &'a Path,
    pub config: &'a mut Config,
    pub mode: &'a mut Mode,

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
    pub script_callbacks: &'a mut ScriptCallbacks,
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
            mode: self.mode,
            next_mode: ModeKind::default(),
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
            script_callbacks: self.script_callbacks,
            tasks: self.tasks,
            lsp: self.lsp,
        };

        (self.scripts, ctx)
    }
}

pub trait ModeState {
    fn on_enter(editor: &mut Editor) {}
    fn on_exit(_ctx: &mut ModeContext) {}
    fn on_client_keys(_ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation;
    fn on_editor_events(_ctx: &mut ModeContext, _events: EditorEventsIter) {}
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeKind {
    Normal,
    Insert,
    ReadLine,
    Picker,
    Script,
}

impl Default for ModeKind {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Default)]
pub struct Mode {
    kind: ModeKind,
    scratch_buf: String,

    pub normal_state: normal::State,
    pub insert_state: insert::State,
    pub read_line_state: read_line::State,
    pub picker_state: picker::State,
    pub script_state: script::State,
}

impl Mode {
    pub fn kind(&self) -> ModeKind {
        self.kind
    }

    pub fn change_to(ctx: &mut ModeContext, next: ModeKind) {
        match ctx.mode.kind {
            ModeKind::Normal => normal::State::on_exit(ctx),
            ModeKind::Insert => insert::State::on_exit(ctx),
            ModeKind::ReadLine => read_line::State::on_exit(ctx),
            ModeKind::Picker => picker::State::on_exit(ctx),
            ModeKind::Script => script::State::on_exit(ctx),
        }

        ctx.mode.kind = next;

        match ctx.mode.kind {
            ModeKind::Normal => normal::State::on_enter(ctx),
            ModeKind::Insert => insert::State::on_enter(ctx),
            ModeKind::ReadLine => read_line::State::on_enter(ctx),
            ModeKind::Picker => picker::State::on_enter(ctx),
            ModeKind::Script => script::State::on_enter(ctx),
        }
    }

    pub fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match ctx.mode.kind {
            ModeKind::Normal => normal::State::on_client_keys(ctx, keys),
            ModeKind::Insert => insert::State::on_client_keys(ctx, keys),
            ModeKind::ReadLine => read_line::State::on_client_keys(ctx, keys),
            ModeKind::Picker => picker::State::on_client_keys(ctx, keys),
            ModeKind::Script => script::State::on_client_keys(ctx, keys),
        }
    }

    pub fn on_editor_events(ctx: &mut ModeContext, events: EditorEventsIter) {
        match ctx.mode.kind {
            ModeKind::Normal => normal::State::on_editor_events(ctx, events),
            ModeKind::Insert => insert::State::on_editor_events(ctx, events),
            ModeKind::ReadLine => read_line::State::on_editor_events(ctx, events),
            ModeKind::Picker => picker::State::on_editor_events(ctx, events),
            ModeKind::Script => script::State::on_editor_events(ctx, events),
        }
    }
}
