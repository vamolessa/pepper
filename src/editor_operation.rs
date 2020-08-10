use std::{error, fmt, path::Path};

use serde::ser;
use serde_derive::{Deserialize, Serialize};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    config::ConfigValues,
    connection::ConnectionWithClientHandle,
    connection::TargetClient,
    cursor::Cursor,
    mode::Mode,
    pattern::Pattern,
    syntax::TokenKind,
    theme::Theme,
};

#[derive(Serialize, Deserialize)]
pub enum EditorOperation<'a> {
    Focused(bool),
    Content,
    Path(Option<&'a Path>),
    Mode(Mode),
    Insert(BufferPosition, &'a str),
    Delete(BufferRange),
    ClearCursors(Cursor),
    Cursor(Cursor),
    InputAppend(char),
    InputKeep(usize),
    Search,
    ConfigValues(ConfigValues),
    Theme(Theme),
    SyntaxExtension(&'a str, &'a str),
    SyntaxRule(&'a str, TokenKind, Pattern),
    Error(&'a str),
}

#[derive(Default)]
pub struct EditorOperationSerializer {
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
}

#[derive(Debug)]
struct SerializationError;
impl fmt::Display for SerializationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(stringify!(SerializationError))
    }
}
impl error::Error for SerializationError {}
impl ser::Error for SerializationError {
    fn custom<T>(_msg: T) -> Self
    where
        T: fmt::Display,
    {
        Self
    }
}

#[derive(Default)]
struct SerializationBuf(Vec<u8>);

impl SerializationBuf {
    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), SerializationError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

impl<'a> ser::Serializer for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(v as _)
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.push_bytes(&v.to_le_bytes())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let mut utf8_encoded_char = [0; std::mem::size_of::<char>()];
        v.encode_utf8(&mut utf8_encoded_char);
        self.push_bytes(&utf8_encoded_char)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.serialize_bytes(v.as_bytes())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(v.len() as _)?;
        self.push_bytes(v)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_bool(false)
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        self.serialize_bool(true)?;
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(variant_index)
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        self.serialize_u32(variant_index)?;
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        match len {
            Some(len) => {
                self.serialize_u32(len as _)?;
                Ok(self)
            }
            None => Err(SerializationError),
        }
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.serialize_u32(variant_index as _)?;
        Ok(self)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        match len {
            Some(len) => {
                self.serialize_u32(len as _)?;
                Ok(self)
            }
            None => Err(SerializationError),
        }
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.serialize_u32(variant_index as _)?;
        Ok(self)
    }
}

impl<'a> ser::SerializeSeq for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> ser::SerializeTuple for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> ser::SerializeTupleStruct for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> ser::SerializeTupleVariant for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> ser::SerializeMap for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        key.serialize(&mut **self)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> ser::SerializeStruct for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> ser::SerializeStructVariant for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerializationError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}
