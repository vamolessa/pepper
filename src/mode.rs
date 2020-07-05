use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    event::Key,
    viewport::ViewportCollection,
};

mod command;
mod insert;
mod normal;
mod search;
mod select;

pub enum Operation {
    None,
    Pending,
    NextViewport,
    EnterMode(Mode),
    LeaveMode,
}

pub enum Mode {
    Normal,
    Select,
    Insert,
    Search,
    Command,
}

pub struct ModeContext<'a> {
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
            Mode::Command => command::on_enter(context),
        }
    }

    pub fn on_event(&mut self, context: ModeContext) -> Operation {
        match self {
            Mode::Normal => normal::on_event(context),
            Mode::Select => select::on_event(context),
            Mode::Insert => insert::on_event(context),
            Mode::Search => search::on_event(context),
            Mode::Command => command::on_event(context),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

pub enum InputResult {
    Pending,
    Submited,
    Canceled,
}

pub fn poll_input(ctx: &mut ModeContext) -> InputResult {
    match ctx.keys {
        [Key::Esc] | [Key::Ctrl('c')] => {
            ctx.input.clear();
            InputResult::Canceled
        }
        [Key::Ctrl('m')] => InputResult::Submited,
        [Key::Ctrl('u')] => {
            ctx.input.clear();
            InputResult::Pending
        }
        [Key::Ctrl('w')] => {
            let mut found_space = false;
            let mut last_index = 0;
            for (i, c) in ctx.input.char_indices().rev() {
                if found_space {
                    if c != ' ' {
                        break;
                    }
                } else {
                    if c == ' ' {
                        found_space = true;
                    }
                }
                last_index = i;
            }

            ctx.input.drain(last_index..);
            InputResult::Pending
        }
        [Key::Ctrl('h')] => {
            if let Some((last_char_index, _)) = ctx.input.char_indices().rev().next() {
                ctx.input.drain(last_char_index..);
            }
            InputResult::Pending
        }
        [Key::Char(c)] => {
            ctx.input.push(*c);
            InputResult::Pending
        }
        _ => InputResult::Pending,
    }
}
