use crate::{buffer::BufferPosition, mode::Mode};

pub struct Undo {}

pub enum Text {
    Char(char),
    String(String),
}

pub enum UndoCommand {
    Insert(Text, Vec<BufferPosition>),
    Delete(Vec<(BufferPosition, BufferPosition)>),
}
