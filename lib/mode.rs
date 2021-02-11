use crate::{
    client::{ClientManager, ClientHandle},
    editor::{Editor, KeysIterator},
    register::RegisterKey,
};

mod command;
mod insert;
mod normal;
pub mod picker;
pub mod read_line;

pub enum ModeOperation {
    Pending,
    Quit,
    QuitAll,
    ExecuteMacro(RegisterKey),
}

pub trait ModeState {
    fn on_enter(_editor: &mut Editor, _clients: &mut ClientManager, _client_handle: ClientHandle) {}
    fn on_exit(_editor: &mut Editor, _clients: &mut ClientManager, _client_handle: ClientHandle) {}
    fn on_client_keys(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<ModeOperation>;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeKind {
    Normal,
    Insert,
    Command,
    ReadLine,
    Picker,
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
    pub command_state: command::State,
    pub read_line_state: read_line::State,
    pub picker_state: picker::State,
}

impl Mode {
    pub fn kind(&self) -> ModeKind {
        self.kind
    }

    pub fn change_to(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        next: ModeKind,
    ) {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_exit(editor, clients, client_handle),
            ModeKind::Insert => insert::State::on_exit(editor, clients, client_handle),
            ModeKind::Command => command::State::on_exit(editor, clients, client_handle),
            ModeKind::ReadLine => read_line::State::on_exit(editor, clients, client_handle),
            ModeKind::Picker => picker::State::on_exit(editor, clients, client_handle),
        }

        editor.mode.kind = next;

        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_enter(editor, clients, client_handle),
            ModeKind::Insert => insert::State::on_enter(editor, clients, client_handle),
            ModeKind::Command => command::State::on_enter(editor, clients, client_handle),
            ModeKind::ReadLine => read_line::State::on_enter(editor, clients, client_handle),
            ModeKind::Picker => picker::State::on_enter(editor, clients, client_handle),
        }
    }

    pub fn on_client_keys(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<ModeOperation> {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_client_keys(editor, clients, client_handle, keys),
            ModeKind::Insert => insert::State::on_client_keys(editor, clients, client_handle, keys),
            ModeKind::Command => command::State::on_client_keys(editor, clients, client_handle, keys),
            ModeKind::ReadLine => read_line::State::on_client_keys(editor, clients, client_handle, keys),
            ModeKind::Picker => picker::State::on_client_keys(editor, clients, client_handle, keys),
        }
    }
}
