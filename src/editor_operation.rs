use std::path::Path;

use serde_derive::{Deserialize, Serialize};

use crate::{
    buffer::{BufferContent, TextRef},
    buffer_position::{BufferPosition, BufferRange},
    config::ConfigValues,
    connection::ConnectionWithClientHandle,
    connection::TargetClient,
    cursor::{Cursor, CursorCollection},
    mode::Mode,
    pattern::Pattern,
    serialization::{DeserializationSlice, SerializationBuf},
    syntax::{Syntax, TokenKind},
    theme::Theme,
};

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum StatusMessageKind {
    Info,
    Error,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EditorOperation<'a> {
    Focused(bool),
    Buffer(&'a str),
    Path(&'a Path),
    Mode(Mode),
    Insert(BufferPosition, &'a str),
    Delete(BufferRange),
    CursorsClear(Cursor),
    Cursor(Cursor),
    InputAppend(char),
    InputKeep(usize),
    Search,
    ConfigValues(&'a [u8]),
    Theme(&'a [u8]),
    SyntaxExtension(&'a str, &'a str),
    SyntaxRule(&'a [u8]),
    SelectClear,
    SelectEntry(&'a str),
    StatusMessage(StatusMessageKind, &'a str),
    StatusMessageAppend(&'a str),
}

#[derive(Default)]
pub struct EditorOperationSerializer {
    temp_buf: SerializationBuf,
    local_buf: SerializationBuf,
    remote_bufs: Vec<SerializationBuf>,
}

impl EditorOperationSerializer {
    pub fn on_client_joined(&mut self, client_handle: ConnectionWithClientHandle) {
        let index = client_handle.into_index();
        if index >= self.remote_bufs.len() {
            self.remote_bufs
                .resize_with(index + 1, || Default::default());
        }
    }

    pub fn on_client_left(&mut self, client_handle: ConnectionWithClientHandle) {
        self.remote_bufs[client_handle.into_index()] = Default::default();
    }

    pub fn serialize(&mut self, target_client: TargetClient, operation: &EditorOperation) {
        use serde::Serialize;
        match target_client {
            TargetClient::All => {
                let _ = operation.serialize(&mut self.local_buf);
                for buf in &mut self.remote_bufs {
                    let _ = operation.serialize(buf);
                }
            }
            TargetClient::Local => {
                let _ = operation.serialize(&mut self.local_buf);
            }
            TargetClient::Remote(handle) => {
                let _ = operation.serialize(&mut self.remote_bufs[handle.into_index()]);
            }
        };
    }

    pub fn serialize_error(&mut self, error: &str) {
        let op = EditorOperation::StatusMessage(StatusMessageKind::Error, error);
        self.serialize(TargetClient::All, &op);
    }

    pub fn serialize_buffer(&mut self, target_client: TargetClient, content: &BufferContent) {
        use serde::Serialize;
        fn write_buffer(buf: &mut SerializationBuf, content: &BufferContent) {
            let _ = EditorOperation::Buffer("").serialize(&mut *buf);
            let content_start = buf.as_slice().len();
            let _ = content.write(&mut *buf);
            let content_len = (buf.as_slice().len() - content_start) as u32;
            let content_len_bytes = content_len.to_le_bytes();

            let len_start = content_start - content_len_bytes.len();
            buf.as_slice_mut()[len_start..(len_start + content_len_bytes.len())]
                .clone_from_slice(&content_len_bytes[..]);
        }

        match target_client {
            TargetClient::All => {
                write_buffer(&mut self.local_buf, content);
                for buf in &mut self.remote_bufs {
                    write_buffer(buf, content);
                }
            }
            TargetClient::Local => write_buffer(&mut self.local_buf, content),
            TargetClient::Remote(handle) => {
                write_buffer(&mut self.remote_bufs[handle.into_index()], content)
            }
        }
    }

    pub fn serialize_insert(
        &mut self,
        target_client: TargetClient,
        position: BufferPosition,
        text: TextRef,
    ) {
        match text {
            TextRef::Char(c) => {
                let mut buf = [0; std::mem::size_of::<char>()];
                let s = c.encode_utf8(&mut buf);
                self.serialize(target_client, &EditorOperation::Insert(position, s));
            }
            TextRef::Str(s) => self.serialize(target_client, &EditorOperation::Insert(position, s)),
        }
    }

    pub fn serialize_cursors(&mut self, target_client: TargetClient, cursors: &CursorCollection) {
        self.serialize(
            target_client,
            &EditorOperation::CursorsClear(*cursors.main_cursor()),
        );
        for cursor in &cursors[..] {
            self.serialize(target_client, &EditorOperation::Cursor(*cursor));
        }
    }

    fn temp_buf_scope<F>(&mut self, callback: F)
    where
        F: FnOnce(&mut EditorOperationSerializer, &mut SerializationBuf),
    {
        let mut temp_buf = SerializationBuf::from_buf(Vec::new());
        std::mem::swap(&mut self.temp_buf, &mut temp_buf);
        callback(self, &mut temp_buf);
        temp_buf.clear();
        std::mem::swap(&mut self.temp_buf, &mut temp_buf);
    }

    pub fn serialize_config_values(
        &mut self,
        target_client: TargetClient,
        config_values: &ConfigValues,
    ) {
        use serde::Serialize;
        self.temp_buf_scope(|this, temp_buf| {
            let _ = config_values.serialize(&mut *temp_buf);
            this.serialize(
                target_client,
                &EditorOperation::ConfigValues(temp_buf.as_slice()),
            );
        });
    }

    pub fn serialize_theme(&mut self, target_client: TargetClient, theme: &Theme) {
        use serde::Serialize;
        self.temp_buf_scope(|this, temp_buf| {
            let _ = theme.serialize(&mut *temp_buf);
            this.serialize(target_client, &EditorOperation::Theme(temp_buf.as_slice()));
        });
    }

    pub fn serialize_syntax_rule(
        &mut self,
        target_client: TargetClient,
        main_extension: &str,
        token_kind: TokenKind,
        pattern: &Pattern,
    ) {
        use serde::Serialize;
        self.temp_buf_scope(|this, temp_buf| {
            let _ = (main_extension, token_kind, pattern).serialize(&mut *temp_buf);
            this.serialize(
                target_client,
                &EditorOperation::SyntaxRule(temp_buf.as_slice()),
            );
        });
    }

    pub fn serialize_syntax(&mut self, target_client: TargetClient, syntax: &Syntax) {
        let mut extensions = syntax.extensions();
        let main_extension = match extensions.next() {
            Some(ext) => ext,
            None => return,
        };

        for ext in extensions {
            self.serialize(
                target_client,
                &EditorOperation::SyntaxExtension(main_extension, ext),
            );
        }

        for (token_kind, pattern) in syntax.rules() {
            self.serialize_syntax_rule(target_client, main_extension, token_kind, pattern);
        }
    }

    pub fn local_bytes(&self) -> &[u8] {
        self.local_buf.as_slice()
    }

    pub fn remote_bytes(&self, handle: ConnectionWithClientHandle) -> &[u8] {
        self.remote_bufs[handle.into_index()].as_slice()
    }

    pub fn clear(&mut self) {
        self.local_buf.clear();
        for buf in &mut self.remote_bufs {
            buf.clear();
        }
    }
}

#[derive(Debug)]
pub enum EditorOperationDeserializeResult<'a> {
    Some(EditorOperation<'a>),
    None,
    Error,
}

pub struct EditorOperationDeserializer<'a>(DeserializationSlice<'a>);

impl<'a> EditorOperationDeserializer<'a> {
    pub fn deserialize_inner<'de, T>(slice: &'de [u8]) -> Option<T>
    where
        T: serde::Deserialize<'de>,
    {
        let mut deserializer = DeserializationSlice::from_slice(slice);
        T::deserialize(&mut deserializer).ok()
    }

    pub fn from_slice(slice: &'a [u8]) -> Self {
        Self(DeserializationSlice::from_slice(slice))
    }

    pub fn deserialize_next(&mut self) -> EditorOperationDeserializeResult<'a> {
        use serde::Deserialize;
        if self.0.as_slice().is_empty() {
            return EditorOperationDeserializeResult::None;
        }

        match EditorOperation::deserialize(&mut self.0) {
            Ok(op) => EditorOperationDeserializeResult::Some(op),
            Err(_) => EditorOperationDeserializeResult::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::syntax::SyntaxCollection;

    macro_rules! assert_next {
        ($d:ident, $p:pat) => {
            let result = $d.deserialize_next();
            if matches!(result, EditorOperationDeserializeResult::Some($p)) {
                assert!(true);
            } else {
                eprintln!("expected: {}\ngot {:?}", stringify!($p), result);
                assert!(false);
            }
        };
    }

    #[test]
    fn buffer_content_serialization() {
        let buffer = BufferContent::from_str("this is some\nbuffer content");
        let mut serializer = EditorOperationSerializer::default();
        serializer.serialize_buffer(TargetClient::Local, &buffer);

        let mut deserializer = EditorOperationDeserializer::from_slice(serializer.local_bytes());
        assert_next!(
            deserializer,
            EditorOperation::Buffer("this is some\nbuffer content")
        );
    }

    #[test]
    fn editor_operation_serialization() {
        let mut serializer = EditorOperationSerializer::default();
        serializer.serialize(TargetClient::Local, &EditorOperation::Focused(true));
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Buffer("this is a content"),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Path(Path::new("this/is/a/path")),
        );
        serializer.serialize(TargetClient::Local, &EditorOperation::Mode(Mode::Insert));
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Insert(BufferPosition::line_col(4, 7), "this is a text"),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Delete(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(2, 3),
            )),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::CursorsClear(Cursor {
                anchor: BufferPosition::line_col(4, 5),
                position: BufferPosition::line_col(6, 7),
            }),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Cursor(Cursor {
                anchor: BufferPosition::line_col(8, 9),
                position: BufferPosition::line_col(10, 11),
            }),
        );
        serializer.serialize(TargetClient::Local, &EditorOperation::InputAppend('h'));
        serializer.serialize(TargetClient::Local, &EditorOperation::InputKeep(12));
        serializer.serialize(TargetClient::Local, &EditorOperation::Search);
        serializer.serialize_config_values(TargetClient::Local, &ConfigValues::default());
        let mut syntax_collection = SyntaxCollection::default();
        let syntax = syntax_collection.get_by_extension("abc");
        syntax.add_extension("def".into());
        syntax.add_rule(TokenKind::Text, Pattern::new("pat").unwrap());
        serializer.serialize_syntax(TargetClient::Local, syntax);
        serializer.serialize_error("this is an error");

        let mut deserializer = EditorOperationDeserializer::from_slice(serializer.local_bytes());

        assert_next!(deserializer, EditorOperation::Focused(true));
        assert_next!(deserializer, EditorOperation::Buffer("this is a content"));
        assert_next!(deserializer, EditorOperation::Path(Path { .. }));
        assert_next!(deserializer, EditorOperation::Mode(Mode::Insert));
        assert_next!(
            deserializer,
            EditorOperation::Insert(
                BufferPosition {
                    line_index: 4,
                    column_index: 7,
                },
                "this is a text"
            )
        );
        assert_next!(deserializer, EditorOperation::Delete(BufferRange { .. }));
        assert_next!(
            deserializer,
            EditorOperation::CursorsClear(Cursor {
                anchor: BufferPosition {
                    line_index: 4,
                    column_index: 5,
                },
                position: BufferPosition {
                    line_index: 6,
                    column_index: 7,
                }
            })
        );
        assert_next!(
            deserializer,
            EditorOperation::Cursor(Cursor {
                anchor: BufferPosition {
                    line_index: 8,
                    column_index: 9,
                },
                position: BufferPosition {
                    line_index: 10,
                    column_index: 11,
                }
            })
        );
        assert_next!(deserializer, EditorOperation::InputAppend('h'));
        assert_next!(deserializer, EditorOperation::InputKeep(12));
        assert_next!(deserializer, EditorOperation::Search);
        assert_next!(deserializer, EditorOperation::ConfigValues(_));
        assert_next!(deserializer, EditorOperation::SyntaxExtension("abc", "def"));
        assert_next!(deserializer, EditorOperation::SyntaxRule(_));
        assert_next!(
            deserializer,
            EditorOperation::StatusMessage(StatusMessageKind::Error, "this is an error")
        );
    }
}
