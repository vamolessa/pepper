use std::path::PathBuf;

use crate::{
    buffer::BufferContent, buffer_position::BufferRange, config::Config, cursor::Cursor,
    editor::EditorOperation, mode::Mode,
};

pub struct Client {
    pub config: Config,

    pub mode: Mode,

    pub path: Option<PathBuf>,
    pub buffer: BufferContent,

    pub main_cursor: Cursor,
    pub cursors: Vec<Cursor>,
    pub search_ranges: Vec<BufferRange>,

    pub has_focus: bool,
    pub input: String,
}

impl Client {
    pub fn new() -> Self {
        Self {
            config: Config::default(),

            mode: Mode::default(),

            path: None,
            buffer: BufferContent::from_str(""),

            main_cursor: Cursor::default(),
            cursors: Vec::new(),
            search_ranges: Vec::new(),

            has_focus: true,
            input: String::new(),
        }
    }

    pub fn on_editor_operation(&mut self, operation: EditorOperation, content: &str) {
        match operation {
            EditorOperation::Focused(focused) => self.has_focus = focused,
            EditorOperation::Content => self.buffer = BufferContent::from_str(content),
            EditorOperation::Path(path) => self.path = path,
            EditorOperation::Mode(mode) => self.mode = mode,
            EditorOperation::Insert(position, text) => {
                self.buffer.insert_text(position, text.as_text_ref());
            }
            EditorOperation::Delete(range) => {
                self.buffer.delete_range(range);
            }
            EditorOperation::ClearCursors(cursor) => {
                self.main_cursor = cursor;
                self.cursors.clear();
            }
            EditorOperation::Cursor(cursor) => self.cursors.push(cursor),
            EditorOperation::SearchAppend(c) => {
                self.input.push(c);
                self.search_ranges.clear();
                self.buffer
                    .find_search_ranges(&self.input[..], &mut self.search_ranges);
            }
            EditorOperation::SearchKeep(keep_count) => {
                self.input.drain(..keep_count);
                self.search_ranges.clear();
                self.buffer
                    .find_search_ranges(&self.input[..], &mut self.search_ranges);
            }
        }
    }
}
