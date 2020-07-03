use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    event::Key,
};

mod insert;
mod normal;
mod select;
mod search;

pub enum Operation {
    None,
    Waiting,
    Exit,
    NextViewport,
    EnterMode(Mode),
}

pub enum Mode {
    Normal,
    Select,
    Insert,
    Search,
}

pub struct ModeContext<'a> {
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub current_buffer_view_handle: Option<&'a BufferViewHandle>,
    pub keys: &'a [Key],
    pub input: &'a mut String,
}

impl Mode {
    pub fn on_enter(&mut self, context: ModeContext) {
        match self {
            Mode::Normal => normal::on_enter(context),
            Mode::Select => select::on_enter(context),
            Mode::Insert => insert::on_enter(context),
            Mode::Search => search::on_enter(context),
        }
    }

    pub fn on_leave(&mut self, context: ModeContext) {
        match self {
            Mode::Normal => normal::on_leave(context),
            Mode::Select => select::on_leave(context),
            Mode::Insert => insert::on_leave(context),
            Mode::Search => search::on_leave(context),
        }
    }

    pub fn on_event(&mut self, context: ModeContext) -> Operation {
        match self {
            Mode::Normal => normal::on_event(context),
            Mode::Select => select::on_event(context),
            Mode::Insert => insert::on_event(context),
            Mode::Search => search::on_event(context),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}
