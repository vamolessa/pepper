use std::{error, fmt, path::Path};

use serde::{de, ser};
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

#[derive(Debug, Serialize, Deserialize)]
pub enum EditorOperation<'a> {
    Focused(bool),
    Content(&'a str),
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

    pub fn local_bytes(&self) -> &[u8] {
        &self.local_buf.0[..]
    }
}

pub struct EditorOperationDeserializer<'a> {
    buf: &'a [u8],
}

impl<'a> EditorOperationDeserializer<'a> {
    pub fn from_slice(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub fn deserialize_next(&mut self) -> Result<Option<EditorOperation<'a>>, ()> {
        use serde::Deserialize;
        if self.buf.len() > 0 {
            let mut deserializer = DeserializationSlice(self.buf);
            match EditorOperation::deserialize(&mut deserializer) {
                Ok(op) => {
                    self.buf = deserializer.0;
                    Ok(Some(op))
                }
                Err(_) => Err(()),
            }
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug)]
struct SerdeError(());
impl SerdeError {
    pub fn new() -> Self {
        panic!("serde error");
        Self(())
    }
}
impl fmt::Display for SerdeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(stringify!(SerializationError))
    }
}
impl error::Error for SerdeError {}
impl ser::Error for SerdeError {
    fn custom<T>(_msg: T) -> Self
    where
        T: fmt::Display,
    {
        Self::new()
    }
}
impl de::Error for SerdeError {
    fn custom<T>(_msg: T) -> Self
    where
        T: fmt::Display,
    {
        eprintln!("msg: {}", _msg);
        Self::new()
    }
}

#[derive(Default)]
struct SerializationBuf(Vec<u8>);

impl SerializationBuf {
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), SerdeError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

macro_rules! impl_serialize_num {
    ($func:ident, $type:ty) => {
        fn $func(self, v: $type) -> Result<Self::Ok, Self::Error> {
            self.write_bytes(&v.to_le_bytes())
        }
    };
}

impl<'a> ser::Serializer for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerdeError;
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

    impl_serialize_num!(serialize_i8, i8);
    impl_serialize_num!(serialize_i16, i16);
    impl_serialize_num!(serialize_i32, i32);
    impl_serialize_num!(serialize_i64, i64);

    impl_serialize_num!(serialize_u8, u8);
    impl_serialize_num!(serialize_u16, u16);
    impl_serialize_num!(serialize_u32, u32);
    impl_serialize_num!(serialize_u64, u64);

    impl_serialize_num!(serialize_f32, f32);
    impl_serialize_num!(serialize_f64, f64);

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let mut buf = [0; std::mem::size_of::<char>()];
        self.serialize_str(v.encode_utf8(&mut buf))
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.serialize_bytes(v.as_bytes())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(v.len() as _)?;
        self.write_bytes(v)
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
            None => Err(SerdeError::new()),
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
        self.serialize_u32(variant_index)?;
        Ok(self)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        match len {
            Some(len) => {
                self.serialize_u32(len as _)?;
                Ok(self)
            }
            None => Err(SerdeError::new()),
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
        self.serialize_u32(variant_index)?;
        Ok(self)
    }
}

impl<'a> ser::SerializeSeq for &'a mut SerializationBuf {
    type Ok = ();
    type Error = SerdeError;

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
    type Error = SerdeError;

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
    type Error = SerdeError;

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
    type Error = SerdeError;

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
    type Error = SerdeError;

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
    type Error = SerdeError;

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
    type Error = SerdeError;

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

struct DeserializationSlice<'de>(&'de [u8]);

macro_rules! read {
    ($read:expr, $type:ty) => {{
        let mut buf = [0; std::mem::size_of::<$type>()];
        let slice = $read.read_bytes(buf.len())?;
        buf.clone_from_slice(slice);
        <$type>::from_le_bytes(buf)
    }};
}

impl<'de> DeserializationSlice<'de> {
    fn read_bytes(&mut self, len: usize) -> Result<&'de [u8], SerdeError> {
        if len <= self.0.len() {
            let mut tmp = &[][..];
            std::mem::swap(&mut tmp, &mut self.0);
            let (before, after) = tmp.split_at(len);
            self.0 = after;
            Ok(before)
        } else {
            dbg!(len, self.0.len());
            Err(SerdeError::new())
        }
    }
}

impl<'de, 'a> de::Deserializer<'de> for &'a mut DeserializationSlice<'de> {
    type Error = SerdeError;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        dbg!("deserialize_any");
        Err(SerdeError::new())
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_bool(read!(self, u8) != 0)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i8(read!(self, i8))
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i16(read!(self, i16))
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i32(read!(self, i32))
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i64(read!(self, i64))
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u8(read!(self, u8))
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u16(read!(self, u16))
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u32(read!(self, u32))
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u64(read!(self, u64))
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_f32(read!(self, f32))
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_f64(read!(self, f64))
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let len = read!(self, u32) as _;
        let slice = self.read_bytes(len)?;
        let s = std::str::from_utf8(slice).map_err(|_| SerdeError::new())?;
        match s.chars().next() {
            Some(c) => visitor.visit_char(c),
            None => Err(SerdeError::new()),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let len = read!(self, u32) as _;
        let slice = self.read_bytes(len)?;
        let s = std::str::from_utf8(slice).map_err(|_| {
            dbg!(slice);
            SerdeError::new()
        })?;
        visitor.visit_borrowed_str(s)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let len = read!(self, u32) as _;
        let slice = self.read_bytes(len)?;
        visitor.visit_borrowed_bytes(slice)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if read!(self, u8) != 0 {
            visitor.visit_some(self)
        } else {
            visitor.visit_none()
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let len = read!(self, u32) as _;
        visitor.visit_seq(DeserializationCollectionAccess { de: self, len })
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        dbg!("deserialize tuple");
        visitor.visit_seq(DeserializationCollectionAccess { de: self, len })
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_seq(DeserializationCollectionAccess { de: self, len })
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let len = read!(self, u32) as _;
        visitor.visit_map(DeserializationCollectionAccess { de: self, len })
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_seq(DeserializationCollectionAccess {
            de: self,
            len: fields.len(),
        })
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        dbg!("deserialize enum");
        visitor.visit_enum(DeserializationEnumAccess { de: self })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        dbg!("deserialize_ignored_any");
        Err(SerdeError::new())
    }
}

struct DeserializationCollectionAccess<'a, 'de: 'a> {
    de: &'a mut DeserializationSlice<'de>,
    len: usize,
}

impl<'de, 'a> de::SeqAccess<'de> for DeserializationCollectionAccess<'a, 'de> {
    type Error = SerdeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.len > 0 {
            dbg!("deserializing seq", self.len);
            self.len -= 1;
            seed.deserialize(&mut *self.de).map(Some)
        } else {
            dbg!("finish seq");
            Ok(None)
        }
    }
}

impl<'de, 'a> de::MapAccess<'de> for DeserializationCollectionAccess<'a, 'de> {
    type Error = SerdeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        if self.len > 0 {
            self.len -= 1;
            seed.deserialize(&mut *self.de).map(Some)
        } else {
            Ok(None)
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }
}

struct DeserializationEnumAccess<'a, 'de: 'a> {
    de: &'a mut DeserializationSlice<'de>,
}

impl<'de, 'a> de::EnumAccess<'de> for DeserializationEnumAccess<'a, 'de> {
    type Error = SerdeError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        use de::IntoDeserializer;
        let variant_index = read!(self.de, u32);
        let value = seed.deserialize(variant_index.into_deserializer())?;
        Ok((value, self))
    }
}

impl<'de, 'a> de::VariantAccess<'de> for DeserializationEnumAccess<'a, 'de> {
    type Error = SerdeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self.de, len, visitor)
    }

    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_struct(self.de, "", fields, visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_next {
        ($d:ident, $p:pat) => {
            let result = $d.deserialize_next();
            if matches!(result, Ok(Some($p))) {
                assert!(true);
            } else {
                eprintln!("expected: {}\ngot {:?}", stringify!($p), result);
                result.unwrap().unwrap();
                assert!(false);
            }
        };
    }

    #[test]
    fn test_editor_operation_serialization() {
        let mut serializer = EditorOperationSerializer::default();
        serializer.serialize(TargetClient::Local, &EditorOperation::Focused(true));
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Content("this is a content"),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Path(Some(Path::new("this/is/a/path"))),
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
            &EditorOperation::ClearCursors(Cursor {
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
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::ConfigValues(ConfigValues::default()),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::SyntaxExtension("abc", "def"),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::SyntaxRule("abc", TokenKind::Text, Pattern::new("pat").unwrap()),
        );
        serializer.serialize(
            TargetClient::Local,
            &EditorOperation::Error("this is an error"),
        );

        let mut deserializer = EditorOperationDeserializer::from_slice(serializer.local_bytes());

        assert_next!(deserializer, EditorOperation::Focused(true));
        assert_next!(deserializer, EditorOperation::Content("this is a content"));
        assert_next!(deserializer, EditorOperation::Path(Some(Path {..})));
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
    }
}
