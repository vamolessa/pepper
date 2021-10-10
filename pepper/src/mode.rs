use crate::{
    client::ClientHandle,
    editor::{Editor, EditorContext, EditorControlFlow, KeysIterator},
};

mod command;
mod insert;
mod normal;
pub(crate) mod picker;
pub(crate) mod read_line;

pub(crate) trait ModeState {
    fn on_enter(editor: &mut Editor);
    fn on_exit(editor: &mut Editor);
    fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorControlFlow>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    pub(crate) fn change_to(editor: &mut Editor, next: ModeKind) {
        if editor.mode.kind == next {
            return;
        }

        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_exit(editor),
            ModeKind::Insert => insert::State::on_exit(editor),
            ModeKind::Command => command::State::on_exit(editor),
            ModeKind::ReadLine => read_line::State::on_exit(editor),
            ModeKind::Picker => picker::State::on_exit(editor),
        }

        editor.mode.kind = next;

        match editor.mode.kind {
            ModeKind::Normal => normal::State::on_enter(editor),
            ModeKind::Insert => insert::State::on_enter(editor),
            ModeKind::Command => command::State::on_enter(editor),
            ModeKind::ReadLine => read_line::State::on_enter(editor),
            ModeKind::Picker => picker::State::on_enter(editor),
        }
    }

    pub(crate) fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorControlFlow> {
        match ctx.editor.mode.kind {
            ModeKind::Normal => normal::State::on_client_keys(ctx, client_handle, keys),
            ModeKind::Insert => insert::State::on_client_keys(ctx, client_handle, keys),
            ModeKind::Command => command::State::on_client_keys(ctx, client_handle, keys),
            ModeKind::ReadLine => read_line::State::on_client_keys(ctx, client_handle, keys),
            ModeKind::Picker => picker::State::on_client_keys(ctx, client_handle, keys),
        }
    }
}
