use std::mem::Discriminant;

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    command::CommandCollection,
    connection::TargetClient,
    editor::{EditorOperationSink, KeysIterator},
    event::Key,
};

mod command;
mod insert;
mod normal;
mod search;
mod select;

pub enum ModeOperation {
    Pending,
    None,
    Quit,
    EnterMode(Mode),
    Error(String),
}

pub struct ModeContext<'a> {
    pub target_client: TargetClient,
    pub operations: &'a mut EditorOperationSink,

    pub commands: &'a CommandCollection,
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub current_buffer_view_handle: &'a mut Option<BufferViewHandle>,
    pub input: &'a mut String,
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
    pub fn discriminant(&self) -> Discriminant<Self> {
        std::mem::discriminant(self)
    }

    pub fn on_enter(&mut self, context: &mut ModeContext) {
        match self {
            Mode::Normal => normal::on_enter(context),
            Mode::Select => select::on_enter(context),
            Mode::Insert => insert::on_enter(context),
            Mode::Search(_) => search::on_enter(context),
            Mode::Command(_) => command::on_enter(context),
        }
    }

    pub fn on_event(
        &mut self,
        context: &mut ModeContext,
        keys: &mut KeysIterator,
    ) -> ModeOperation {
        match self {
            Mode::Normal => normal::on_event(context, keys),
            Mode::Select => select::on_event(context, keys),
            Mode::Insert => insert::on_event(context, keys),
            Mode::Search(from_mode) => search::on_event(context, keys, from_mode),
            Mode::Command(from_mode) => command::on_event(context, keys, from_mode),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

pub enum InputPollResult {
    Pending,
    Submited,
    Canceled,
}

pub fn poll_input(ctx: &mut ModeContext, keys: &mut KeysIterator) -> InputPollResult {
    match keys.next() {
        Key::Esc | Key::Ctrl('c') => {
            ctx.input.clear();
            InputPollResult::Canceled
        }
        Key::Ctrl('m') => InputPollResult::Submited,
        Key::Ctrl('u') => {
            ctx.input.clear();
            InputPollResult::Pending
        }
        Key::Ctrl('w') => {
            let mut found_space = false;
            let mut last_index = 0;
            for (i, c) in ctx.input.char_indices().rev() {
                if found_space {
                    if c != ' ' {
                        break;
                    }
                } else if c == ' ' {
                    found_space = true;
                }
                last_index = i;
            }

            ctx.input.drain(last_index..);
            InputPollResult::Pending
        }
        Key::Ctrl('h') => {
            if let Some((last_char_index, _)) = ctx.input.char_indices().rev().next() {
                ctx.input.drain(last_char_index..);
            }
            InputPollResult::Pending
        }
        Key::Char(c) => {
            ctx.input.push(c);
            InputPollResult::Pending
        }
        _ => InputPollResult::Pending,
    }
}
