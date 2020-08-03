use std::path::PathBuf;

use crate::{
    buffer::BufferContent,
    buffer_position::BufferRange,
    config::Config,
    cursor::Cursor,
    editor::EditorOperation,
    mode::Mode,
    syntax::{HighlightedBuffer, SyntaxHandle},
};

pub struct Client<'a> {
    pub config: &'a Config,

    pub mode: Mode,

    pub path: Option<PathBuf>,
    pub buffer: BufferContent,
    pub highlighted_buffer: HighlightedBuffer,
    pub syntax_handle: Option<SyntaxHandle>,

    pub main_cursor: Cursor,
    pub cursors: Vec<Cursor>,
    pub search_ranges: Vec<BufferRange>,

    pub has_focus: bool,
    pub input: String,
    pub error: Option<String>,
}

impl<'a> Client<'a> {
    pub fn with_config(config: &'a Config) -> Self {
        Self {
            config,

            mode: Mode::default(),

            path: None,
            buffer: BufferContent::from_str(""),
            highlighted_buffer: HighlightedBuffer::default(),
            syntax_handle: None,

            main_cursor: Cursor::default(),
            cursors: Vec::new(),
            search_ranges: Vec::new(),

            has_focus: true,
            input: String::new(),
            error: None,
        }
    }

    pub fn on_editor_operation(&mut self, operation: &EditorOperation, content: &str) {
        match operation {
            EditorOperation::Focused(focused) => self.has_focus = *focused,
            EditorOperation::Content => {
                self.buffer = BufferContent::from_str(content);
                self.main_cursor = Cursor::default();
                self.cursors.clear();
                self.cursors.push(self.main_cursor);

                if let Some(handle) = self.syntax_handle {
                    let syntax = self.config.syntaxes.get(handle);
                    self.highlighted_buffer.highligh_all(syntax, &self.buffer);
                }
            }
            EditorOperation::Path(path) => {
                self.path = path.clone();
                self.syntax_handle = None;

                if let Some(extension) = self
                    .path
                    .as_ref()
                    .and_then(|p| p.extension().or(p.file_name()).and_then(|s| s.to_str()))
                {
                    self.syntax_handle = self.config.syntaxes.find_by_extension(extension);
                }

                if let Some(handle) = self.syntax_handle {
                    let syntax = self.config.syntaxes.get(handle);
                    self.highlighted_buffer.highligh_all(syntax, &self.buffer);
                }
            }
            EditorOperation::Mode(mode) => self.mode = mode.clone(),
            EditorOperation::Insert(position, text) => {
                self.buffer.insert_text(*position, text.as_text_ref());
                if let Some(handle) = self.syntax_handle {
                    let syntax = self.config.syntaxes.get(handle);
                    self.highlighted_buffer.highligh_all(syntax, &self.buffer);
                }
            }
            EditorOperation::Delete(range) => {
                self.buffer.delete_range(*range);
                if let Some(handle) = self.syntax_handle {
                    let syntax = self.config.syntaxes.get(handle);
                    self.highlighted_buffer.highligh_all(syntax, &self.buffer);
                }
            }
            EditorOperation::ClearCursors(cursor) => {
                self.main_cursor = *cursor;
                self.cursors.clear();
            }
            EditorOperation::Cursor(cursor) => self.cursors.push(*cursor),
            EditorOperation::InputAppend(c) => self.input.push(*c),
            EditorOperation::InputKeep(keep_count) => {
                self.input.truncate(*keep_count);
            }
            EditorOperation::Search => {
                self.search_ranges.clear();
                self.buffer
                    .find_search_ranges(&self.input[..], &mut self.search_ranges);
            }
            EditorOperation::Error(error) => self.error = Some(error.clone()),
        }
    }
}
