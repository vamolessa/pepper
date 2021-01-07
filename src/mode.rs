use crate::{
    client::ClientCollection,
    editor::{Editor, KeysIterator},
    register::RegisterKey,
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

pub trait ModeState {
    fn on_enter(_editor: &mut Editor, _clients: &mut ClientCollection) {}
    fn on_exit(_editor: &mut Editor, _clients: &mut ClientCollection) {}
    fn on_client_keys(
        _editor: &mut Editor,
        _clients: &mut ClientCollection,
        keys: &mut KeysIterator,
    ) -> ModeOperation;
    fn on_editor_events(_editor: &mut Editor, _clients: &mut ClientCollection) {}
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

    pub fn change_to(editor: &mut Editor, clients: &mut ClientCollection, next: ModeKind) {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_exit(editor, clients),
            ModeKind::Insert => insert::State::on_exit(editor, clients),
            ModeKind::ReadLine => read_line::State::on_exit(editor, clients),
            ModeKind::Picker => picker::State::on_exit(editor, clients),
            ModeKind::Script => script::State::on_exit(editor, clients),
        }

        editor.mode.kind = next;

        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_enter(editor, clients),
            ModeKind::Insert => insert::State::on_enter(editor, clients),
            ModeKind::ReadLine => read_line::State::on_enter(editor, clients),
            ModeKind::Picker => picker::State::on_enter(editor, clients),
            ModeKind::Script => script::State::on_enter(editor, clients),
        }
    }

    pub fn on_client_keys(editor: &mut Editor, clients: &mut ClientCollection, keys: &mut KeysIterator) -> ModeOperation {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_client_keys(editor, clients, keys),
            ModeKind::Insert => insert::State::on_client_keys(editor, clients, keys),
            ModeKind::ReadLine => read_line::State::on_client_keys(editor, clients, keys),
            ModeKind::Picker => picker::State::on_client_keys(editor, clients, keys),
            ModeKind::Script => script::State::on_client_keys(editor, clients, keys),
        }
    }

    pub fn on_editor_events(editor: &mut Editor, clients: &mut ClientCollection) {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_editor_events(editor, clients),
            ModeKind::Insert => insert::State::on_editor_events(editor, clients),
            ModeKind::ReadLine => read_line::State::on_editor_events(editor, clients),
            ModeKind::Picker => picker::State::on_editor_events(editor, clients),
            ModeKind::Script => script::State::on_editor_events(editor, clients),
        }
    }
}
