use crate::{
    buffer::BufferCollection, buffer_view::BufferViewCollection, command::CommandCollection,
    event::Key, viewport::ViewportCollection,
};

mod command;
mod insert;
mod normal;
mod search;
mod select;

pub struct ModeContext<'a> {
    pub commands: &'a CommandCollection,
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub viewports: &'a mut ViewportCollection,
    pub keys: &'a [Key],
    pub input: &'a mut String,
}

pub enum ModeOperation {
    None,
    Pending,
    Quit,
    EnterMode(Mode),
    Error(String),
}

pub enum FromMode {
    Normal,
    Select,
}

impl FromMode {
    pub fn as_mode(&self) -> Mode {
        match self {
            FromMode::Normal => Mode::Normal,
            FromMode::Select => Mode::Select,
        }
    }
}

pub enum Mode {
    Normal,
    Select,
    Insert,
    Search(FromMode),
    Command(FromMode),
}

impl Mode {
    pub fn on_enter(&mut self, context: ModeContext) {
        match self {
            Mode::Normal => normal::on_enter(context),
            Mode::Select => select::on_enter(context),
            Mode::Insert => insert::on_enter(context),
            Mode::Search(_) => search::on_enter(context),
            Mode::Command(_) => command::on_enter(context),
        }
    }

    pub fn on_event(&mut self, context: ModeContext) -> ModeOperation {
        match self {
            Mode::Normal => normal::on_event(context),
            Mode::Select => select::on_event(context),
            Mode::Insert => insert::on_event(context),
            Mode::Search(from_mode) => search::on_event(context, from_mode),
            Mode::Command(from_mode) => command::on_event(context, from_mode),
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
