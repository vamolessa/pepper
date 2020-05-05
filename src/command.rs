use crate::{buffer::BufferPosition, mode::Mode};

pub struct Undo {

}

pub enum ImmediateCommand {
    None,
    Exit,
    EnterMode(Box<dyn Mode>),
    MoveCursors(u16, u16),
}

pub enum UndoCommand {
    InsertChar(char, Vec<BufferPosition>),
    BreakLine(Vec<BufferPosition>),
}
