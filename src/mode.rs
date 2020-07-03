use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    event::Key,
    viewport::ViewportCollection,
};

mod insert;
mod normal;
mod search;
mod select;

pub enum Operation {
    None,
    Waiting,
    Exit,
    NextViewport,
    EnterMode(Mode),
}

#[derive(Clone, Copy)]
pub enum Mode {
    Normal,
    Select,
    Insert,
    Search,
}

pub struct ModeContext<'a> {
    pub previous_mode: Mode,
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub viewports: &'a ViewportCollection,
    pub keys: &'a [Key],
    pub input: &'a mut String,
}

impl<'a> ModeContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<&'a BufferViewHandle> {
        self.viewports
            .current_viewport()
            .current_buffer_view_handle()
    }
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
