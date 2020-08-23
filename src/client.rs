use std::path::PathBuf;

use crate::{
    buffer::{BufferContent, TextRef},
    buffer_position::BufferRange,
    command::{CommandCollection, ConfigCommandContext},
    config::Config,
    cursor::Cursor,
    editor_operation::{
        EditorOperation, EditorOperationDeserializeResult, EditorOperationDeserializer,
        EditorOperationSerializer, StatusMessageKind,
    },
    keymap::KeyMapCollection,
    mode::Mode,
    select::SelectEntryCollection,
    syntax::{HighlightedBuffer, SyntaxHandle},
};

pub struct Client {
    pub config: Config,
    pub mode: Mode,

    pub path: PathBuf,
    pub buffer: BufferContent,
    pub highlighted_buffer: HighlightedBuffer,
    pub syntax_handle: Option<SyntaxHandle>,

    pub main_cursor: Cursor,
    pub cursors: Vec<Cursor>,
    pub search_ranges: Vec<BufferRange>,

    pub has_focus: bool,
    pub input: String,
    pub select_entries: SelectEntryCollection,

    pub status_message_kind: StatusMessageKind,
    pub status_message: String,
}

impl Client {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            mode: Mode::default(),

            path: PathBuf::new(),
            buffer: BufferContent::from_str(""),
            highlighted_buffer: HighlightedBuffer::default(),
            syntax_handle: None,

            main_cursor: Cursor::default(),
            cursors: Vec::new(),
            search_ranges: Vec::new(),

            has_focus: true,
            input: String::new(),
            select_entries: SelectEntryCollection::default(),

            status_message_kind: StatusMessageKind::Info,
            status_message: String::new(),
        }
    }

    pub fn load_config(
        &mut self,
        commands: &CommandCollection,
        keymaps: &mut KeyMapCollection,
        operations: &mut EditorOperationSerializer,
    ) {
        let mut ctx = ConfigCommandContext {
            operations,
            config: &self.config,
            keymaps,
        };

        if let Err(e) = Config::load_into_operations(commands, &mut ctx) {
            self.status_message.clear();
            self.status_message.push_str(&e[..]);
            return;
        }

        let mut deserializer = EditorOperationDeserializer::from_slice(operations.local_bytes());
        loop {
            match deserializer.deserialize_next() {
                EditorOperationDeserializeResult::Some(op) => self.on_editor_operation(&op),
                EditorOperationDeserializeResult::None
                | EditorOperationDeserializeResult::Error => break,
            }
        }
    }

    pub fn on_editor_operation(&mut self, operation: &EditorOperation) {
        match operation {
            EditorOperation::Focused(focused) => self.has_focus = *focused,
            EditorOperation::Buffer(content) => {
                self.search_ranges.clear();
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
                self.path.clear();
                self.path.push(path);

                self.syntax_handle = None;

                if let Some(extension) = self
                    .path
                    .extension()
                    .or(self.path.file_name())
                    .and_then(|s| s.to_str())
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
                self.search_ranges.clear();
                let range = self.buffer.insert_text(*position, TextRef::Str(text));
                if let Some(handle) = self.syntax_handle {
                    let syntax = self.config.syntaxes.get(handle);
                    self.highlighted_buffer
                        .on_insert(syntax, &self.buffer, range);
                }
            }
            EditorOperation::Delete(range) => {
                self.search_ranges.clear();
                self.buffer.delete_range(*range);
                if let Some(handle) = self.syntax_handle {
                    let syntax = self.config.syntaxes.get(handle);
                    self.highlighted_buffer
                        .on_delete(syntax, &self.buffer, *range);
                }
            }
            EditorOperation::CursorsClear(cursor) => {
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
            EditorOperation::ConfigValues(serialized) => {
                if let Some(values) = EditorOperationDeserializer::deserialize_inner(serialized) {
                    self.config.values = values;
                }
            }
            EditorOperation::Theme(serialized) => {
                if let Some(theme) = EditorOperationDeserializer::deserialize_inner(serialized) {
                    self.config.theme = theme;
                }
            }
            EditorOperation::SyntaxExtension(main_extension, other_extension) => self
                .config
                .syntaxes
                .get_by_extension(main_extension)
                .add_extension((*other_extension).into()),
            EditorOperation::SyntaxRule(serialized) => {
                if let Some((main_extension, token_kind, pattern)) =
                    EditorOperationDeserializer::deserialize_inner(serialized)
                {
                    self.config
                        .syntaxes
                        .get_by_extension(main_extension)
                        .add_rule(token_kind, pattern);
                }
            }
            EditorOperation::SelectClear => self.select_entries.clear(),
            EditorOperation::SelectEntry(name) => self.select_entries.add(name),
            EditorOperation::StatusMessage(kind, message) => {
                self.status_message_kind = *kind;
                self.status_message.clear();
                self.status_message.push_str(message);
            }
            EditorOperation::StatusMessageAppend(message) => {
                self.status_message.push_str(message);
            }
        }
    }
}
