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

    pub fn change_to(editor: &mut Editor, next: ModeKind) {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_exit(editor),
            ModeKind::Insert => insert::State::on_exit(editor),
            ModeKind::ReadLine => read_line::State::on_exit(editor),
            ModeKind::Picker => picker::State::on_exit(editor),
            ModeKind::Script => script::State::on_exit(editor),
        }

        editor.mode.kind = next;

        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_enter(editor),
            ModeKind::Insert => insert::State::on_enter(editor),
            ModeKind::ReadLine => read_line::State::on_enter(editor),
            ModeKind::Picker => picker::State::on_enter(editor),
            ModeKind::Script => script::State::on_enter(editor),
        }
    }

    pub fn on_client_keys(editor: &mut Editor, keys: &mut KeysIterator) -> ModeOperation {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_client_keys(editor, keys),
            ModeKind::Insert => insert::State::on_client_keys(editor, keys),
            ModeKind::ReadLine => read_line::State::on_client_keys(editor, keys),
            ModeKind::Picker => picker::State::on_client_keys(editor, keys),
            ModeKind::Script => script::State::on_client_keys(editor, keys),
        }
    }

    pub fn on_editor_events(editor: &mut Editor) {
        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_editor_events(editor),
            ModeKind::Insert => insert::State::on_editor_events(editor),
            ModeKind::ReadLine => read_line::State::on_editor_events(editor),
            ModeKind::Picker => picker::State::on_editor_events(editor),
            ModeKind::Script => script::State::on_editor_events(editor),
        }
    }
}
