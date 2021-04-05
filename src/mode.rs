use crate::{
    client::{ClientHandle, ClientManager},
    command::CommandOperation,
    editor::{Editor, KeysIterator},
    platform::Platform,
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
impl From<CommandOperation> for ModeOperation {
    fn from(op: CommandOperation) -> Self {
        match op {
            CommandOperation::Quit => ModeOperation::Quit,
            CommandOperation::QuitAll => ModeOperation::QuitAll,
        }
    }
}

pub struct ModeContext<'a> {
    pub editor: &'a mut Editor,
    pub platform: &'a mut Platform,
    pub clients: &'a mut ClientManager,
    pub client_handle: ClientHandle,
}

pub trait ModeState {
    fn on_enter(ctx: &mut ModeContext);
    fn on_exit(ctx: &mut ModeContext);
    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation>;
    fn on_buffer_changed(_: &mut ModeContext) {}
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

    pub fn change_to(ctx: &mut ModeContext, next: ModeKind) {
        match ctx.editor.mode.kind {
            ModeKind::Normal => normal::State::on_exit(ctx),
            ModeKind::Insert => insert::State::on_exit(ctx),
            ModeKind::Command => command::State::on_exit(ctx),
            ModeKind::ReadLine => read_line::State::on_exit(ctx),
            ModeKind::Picker => picker::State::on_exit(ctx),
        }

        ctx.editor.mode.kind = next;

        match ctx.editor.mode.kind {
            ModeKind::Normal => normal::State::on_enter(ctx),
            ModeKind::Insert => insert::State::on_enter(ctx),
            ModeKind::Command => command::State::on_enter(ctx),
            ModeKind::ReadLine => read_line::State::on_enter(ctx),
            ModeKind::Picker => picker::State::on_enter(ctx),
        }
    }

    pub fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        match ctx.editor.mode.kind {
            ModeKind::Normal => normal::State::on_client_keys(ctx, keys),
            ModeKind::Insert => insert::State::on_client_keys(ctx, keys),
            ModeKind::Command => command::State::on_client_keys(ctx, keys),
            ModeKind::ReadLine => read_line::State::on_client_keys(ctx, keys),
            ModeKind::Picker => picker::State::on_client_keys(ctx, keys),
        }
    }

    pub fn on_buffer_changed(ctx: &mut ModeContext) {
        match ctx.editor.mode.kind {
            ModeKind::Normal => normal::State::on_buffer_changed(ctx),
            ModeKind::Insert => insert::State::on_buffer_changed(ctx),
            ModeKind::Command => command::State::on_buffer_changed(ctx),
            ModeKind::ReadLine => read_line::State::on_buffer_changed(ctx),
            ModeKind::Picker => picker::State::on_buffer_changed(ctx),
        }
    }
}
