use std::{convert::TryFrom, error, fmt};

use serde::{de, ser};
use serde_derive::{Deserialize, Serialize};

use crate::event_manager::ConnectionEvent;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    None,
    Key(Key),
    Resize(u16, u16),
    Connection(ConnectionEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    None,
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    Delete,
    F(u8),
    Char(char),
    Ctrl(char),
    Alt(char),
    Esc,
}

#[derive(Debug)]
pub enum KeyParseError {
    UnexpectedEnd,
    InvalidCharacter(char),
}

impl fmt::Display for KeyParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::UnexpectedEnd => write!(f, "could not finish parsing key"),
            Self::InvalidCharacter(c) => write!(f, "invalid character {}", c),
        }
    }
}

impl Key {
    pub fn parse(chars: &mut impl Iterator<Item = char>) -> Result<Self, KeyParseError> {
        macro_rules! next {
            () => {
                match chars.next() {
                    Some(element) => element,
                    None => return Err(KeyParseError::UnexpectedEnd),
                }
            };
        }

        macro_rules! consume {
            ($character:expr) => {
                let c = next!();
                if c != $character {
                    return Err(KeyParseError::InvalidCharacter(c));
                }
            };
        }

        macro_rules! consume_str {
            ($str:expr) => {
                for c in $str.chars() {
                    consume!(c);
                }
            };
        }

        let key = match next!() {
            '\\' => match next!() {
                '\\' => Key::Char('\\'),
                '<' => Key::Char('<'),
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            '<' => match next!() {
                'b' => {
                    consume_str!("ackspace>");
                    Key::Backspace
                }
                's' => {
                    consume_str!("pace>");
                    Key::Char(' ')
                }
                'e' => match next!() {
                    'n' => match next!() {
                        't' => {
                            consume_str!("er>");
                            Key::Enter
                        }
                        'd' => {
                            consume!('>');
                            Key::End
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    },
                    's' => {
                        consume_str!("c>");
                        Key::Esc
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'l' => {
                    consume_str!("eft>");
                    Key::Left
                }
                'r' => {
                    consume_str!("ight>");
                    Key::Right
                }
                'u' => {
                    consume_str!("p>");
                    Key::Up
                }
                'd' => match next!() {
                    'o' => {
                        consume_str!("wn>");
                        Key::Down
                    }
                    'e' => {
                        consume_str!("lete>");
                        Key::Delete
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'h' => {
                    consume_str!("ome>");
                    Key::Home
                }
                'p' => {
                    consume_str!("age");
                    match next!() {
                        'u' => {
                            consume_str!("p>");
                            Key::PageUp
                        }
                        'd' => {
                            consume_str!("own>");
                            Key::PageDown
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    }
                }
                't' => {
                    consume_str!("ab>");
                    Key::Tab
                }
                'f' => {
                    let n = match next!() {
                        '1' => match next!() {
                            '>' => 1,
                            '0' => {
                                consume!('>');
                                10
                            }
                            '1' => {
                                consume!('>');
                                11
                            }
                            '2' => {
                                consume!('>');
                                12
                            }
                            c => return Err(KeyParseError::InvalidCharacter(c)),
                        },
                        c => {
                            consume!('>');
                            match c.to_digit(10) {
                                Some(n) => n,
                                None => return Err(KeyParseError::InvalidCharacter(c)),
                            }
                        }
                    };
                    Key::F(n as _)
                }
                'c' => {
                    consume!('-');
                    let c = next!();
                    let key = if c.is_ascii_alphanumeric() {
                        Key::Ctrl(c)
                    } else {
                        return Err(KeyParseError::InvalidCharacter(c));
                    };
                    consume!('>');
                    key
                }
                'a' => {
                    consume!('-');
                    let c = next!();
                    let key = if c.is_ascii_alphanumeric() {
                        Key::Alt(c)
                    } else {
                        return Err(KeyParseError::InvalidCharacter(c));
                    };
                    consume!('>');
                    key
                }
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            c => {
                if c.is_ascii() {
                    Key::Char(c)
                } else {
                    return Err(KeyParseError::InvalidCharacter(c));
                }
            }
        };

        Ok(key)
    }
}

#[derive(Debug)]
pub struct KeySerializationError;
impl fmt::Display for KeySerializationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(stringify!(SerializationError))
    }
}
impl error::Error for KeySerializationError {}
impl ser::Error for KeySerializationError {
    fn custom<T>(_msg: T) -> Self
    where
        T: fmt::Display,
    {
        Self
    }
}
impl de::Error for KeySerializationError {
    fn custom<T>(_msg: T) -> Self
    where
        T: fmt::Display,
    {
        Self
    }
}

#[derive(Default)]
pub struct KeySerializer {
    buf: [u8; 8],
    len: usize,
}

impl KeySerializer {
    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn write_byte(&mut self, byte: u8) {
        self.buf[self.len] = byte;
        self.len += 1;
    }
}

macro_rules! serialize_unreachable {
    ($func:ident,$type:ty) => {
        fn $func(self, _v: $type) -> Result<Self::Ok, Self::Error> {
            unreachable!();
        }
    };
}

impl<'a> ser::Serializer for &'a mut KeySerializer {
    type Ok = ();
    type Error = KeySerializationError;
    type SerializeSeq = ser::Impossible<(), KeySerializationError>;
    type SerializeTuple = ser::Impossible<(), KeySerializationError>;
    type SerializeTupleStruct = ser::Impossible<(), KeySerializationError>;
    type SerializeTupleVariant = Self;
    type SerializeMap = ser::Impossible<(), KeySerializationError>;
    type SerializeStruct = ser::Impossible<(), KeySerializationError>;
    type SerializeStructVariant = ser::Impossible<(), KeySerializationError>;

    serialize_unreachable!(serialize_bool, bool);

    serialize_unreachable!(serialize_i8, i8);
    serialize_unreachable!(serialize_i16, i16);
    serialize_unreachable!(serialize_i32, i32);
    serialize_unreachable!(serialize_i64, i64);

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.write_byte(v);
        Ok(())
    }

    serialize_unreachable!(serialize_u16, u16);
    serialize_unreachable!(serialize_u32, u32);
    serialize_unreachable!(serialize_u64, u64);

    serialize_unreachable!(serialize_f32, f32);
    serialize_unreachable!(serialize_f64, f64);

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let bytes = u32::to_le_bytes(v as _);
        self.write_byte(bytes[0]);
        self.write_byte(bytes[1]);
        self.write_byte(bytes[2]);
        self.write_byte(bytes[3]);
        Ok(())
    }

    serialize_unreachable!(serialize_str, &str);
    serialize_unreachable!(serialize_bytes, &[u8]);

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        unreachable!();
    }

    fn serialize_some<T>(self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        unreachable!();
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        unreachable!();
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        unreachable!();
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(variant_index as _)
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        unreachable!();
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
        self.serialize_u8(variant_index as _)?;
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        unreachable!();
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        unreachable!();
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        unreachable!();
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.serialize_u8(variant_index as _)?;
        Ok(self)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        unreachable!();
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        unreachable!();
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        unreachable!();
    }
}

impl<'a> ser::SerializeTupleVariant for &'a mut KeySerializer {
    type Ok = ();
    type Error = KeySerializationError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

#[derive(Debug)]
pub enum KeyDeserializeResult {
    Some(Key),
    None,
    Error,
}

pub struct KeyDeserializer<'de>(&'de [u8]);

impl<'de> KeyDeserializer<'de> {
    pub fn deserialize_next(&mut self) -> KeyDeserializeResult {
        use serde::Deserialize;
        if self.0.is_empty() {
            return KeyDeserializeResult::None;
        }

        match Key::deserialize(self) {
            Ok(key) => KeyDeserializeResult::Some(key),
            Err(_) => KeyDeserializeResult::Error,
        }
    }

    fn read_byte(&mut self) -> u8 {
        let byte = self.0[0];
        self.0 = &self.0[1..];
        byte
    }
}

macro_rules! deserializations_unreachable {
    ($($func:ident,)*) => {
        $(
            fn $func<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
            where
                V: de::Visitor<'de>,
            {
                unreachable!();
            }
        )*
    }
}

impl<'de, 'a> de::Deserializer<'de> for &'a mut KeyDeserializer<'de> {
    type Error = KeySerializationError;

    deserializations_unreachable!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_f32,
        deserialize_f64,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_identifier,
        deserialize_ignored_any,
    );

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u8(self.read_byte())
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let bytes = [
            self.read_byte(),
            self.read_byte(),
            self.read_byte(),
            self.read_byte(),
        ];
        let c = u32::from_le_bytes(bytes);
        visitor.visit_char(char::try_from(c).map_err(|_| KeySerializationError)?)
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unreachable!();
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unreachable!();
    }

    fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unreachable!();
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_seq(DeserializationCollectionAccess { de: self, len })
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unreachable!();
    }

    fn deserialize_map<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unreachable!();
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unreachable!();
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
        visitor.visit_enum(self)
    }
}

struct DeserializationCollectionAccess<'a, 'de: 'a> {
    de: &'a mut KeyDeserializer<'de>,
    len: usize,
}

impl<'de, 'a> de::SeqAccess<'de> for DeserializationCollectionAccess<'a, 'de> {
    type Error = KeySerializationError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.len > 0 {
            self.len -= 1;
            seed.deserialize(&mut *self.de).map(Some)
        } else {
            Ok(None)
        }
    }
}

impl<'de, 'a> de::EnumAccess<'de> for &'a mut KeyDeserializer<'de> {
    type Error = KeySerializationError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        use de::IntoDeserializer;
        let variant_index = self.read_byte() as u32;
        let value = seed.deserialize(variant_index.into_deserializer())?;
        Ok((value, self))
    }
}

impl<'de, 'a> de::VariantAccess<'de> for &'a mut KeyDeserializer<'de> {
    type Error = KeySerializationError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(self)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        use de::Deserializer;
        self.deserialize_tuple(len, visitor)
    }

    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        use de::Deserializer;
        self.deserialize_struct("", fields, visitor)
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[test]
    fn parse_key() {
        assert_eq!(
            Key::Backspace,
            Key::parse(&mut "<backspace>".chars()).unwrap()
        );
        assert_eq!(Key::Char(' '), Key::parse(&mut "<space>".chars()).unwrap());
        assert_eq!(Key::Enter, Key::parse(&mut "<enter>".chars()).unwrap());
        assert_eq!(Key::Left, Key::parse(&mut "<left>".chars()).unwrap());
        assert_eq!(Key::Right, Key::parse(&mut "<right>".chars()).unwrap());
        assert_eq!(Key::Up, Key::parse(&mut "<up>".chars()).unwrap());
        assert_eq!(Key::Down, Key::parse(&mut "<down>".chars()).unwrap());
        assert_eq!(Key::Home, Key::parse(&mut "<home>".chars()).unwrap());
        assert_eq!(Key::End, Key::parse(&mut "<end>".chars()).unwrap());
        assert_eq!(Key::PageUp, Key::parse(&mut "<pageup>".chars()).unwrap());
        assert_eq!(
            Key::PageDown,
            Key::parse(&mut "<pagedown>".chars()).unwrap()
        );
        assert_eq!(Key::Tab, Key::parse(&mut "<tab>".chars()).unwrap());
        assert_eq!(Key::Delete, Key::parse(&mut "<delete>".chars()).unwrap());
        assert_eq!(Key::Esc, Key::parse(&mut "<esc>".chars()).unwrap());

        for n in 1..=12 {
            let s = format!("<f{}>", n);
            assert_eq!(Key::F(n as _), Key::parse(&mut s.chars()).unwrap());
        }

        assert_eq!(Key::Ctrl('z'), Key::parse(&mut "<c-z>".chars()).unwrap());
        assert_eq!(Key::Ctrl('0'), Key::parse(&mut "<c-0>".chars()).unwrap());
        assert_eq!(Key::Ctrl('9'), Key::parse(&mut "<c-9>".chars()).unwrap());

        assert_eq!(Key::Alt('a'), Key::parse(&mut "<a-a>".chars()).unwrap());
        assert_eq!(Key::Alt('z'), Key::parse(&mut "<a-z>".chars()).unwrap());
        assert_eq!(Key::Alt('0'), Key::parse(&mut "<a-0>".chars()).unwrap());
        assert_eq!(Key::Alt('9'), Key::parse(&mut "<a-9>".chars()).unwrap());

        assert_eq!(Key::Char('a'), Key::parse(&mut "a".chars()).unwrap());
        assert_eq!(Key::Char('z'), Key::parse(&mut "z".chars()).unwrap());
        assert_eq!(Key::Char('0'), Key::parse(&mut "0".chars()).unwrap());
        assert_eq!(Key::Char('9'), Key::parse(&mut "9".chars()).unwrap());
        assert_eq!(Key::Char('_'), Key::parse(&mut "_".chars()).unwrap());
        assert_eq!(Key::Char('<'), Key::parse(&mut "\\<".chars()).unwrap());
        assert_eq!(Key::Char('\\'), Key::parse(&mut "\\\\".chars()).unwrap());
    }

    #[test]
    fn min_key_serializer_buf_size() {
        let serializer = KeySerializer::default();
        assert!(std::mem::size_of::<Key>() <= serializer.buf.len());
    }

    #[test]
    fn key_serialization() {
        macro_rules! assert_serialization {
            ($key:expr) => {
                let mut serializer = KeySerializer::default();
                $key.serialize(&mut serializer).unwrap();
                let mut deserializer = KeyDeserializer(serializer.slice());
                if let KeyDeserializeResult::Some(key) = deserializer.deserialize_next() {
                    assert_eq!($key, key);
                } else {
                    assert!(false);
                }
            };
        }

        assert_serialization!(Key::None);
        assert_serialization!(Key::Backspace);
        assert_serialization!(Key::Enter);
        assert_serialization!(Key::Left);
        assert_serialization!(Key::Right);
        assert_serialization!(Key::Up);
        assert_serialization!(Key::Down);
        assert_serialization!(Key::Home);
        assert_serialization!(Key::End);
        assert_serialization!(Key::PageUp);
        assert_serialization!(Key::PageDown);
        assert_serialization!(Key::Tab);
        assert_serialization!(Key::Delete);
        assert_serialization!(Key::F(0));
        assert_serialization!(Key::F(9));
        assert_serialization!(Key::F(12));
        assert_serialization!(Key::Char('a'));
        assert_serialization!(Key::Char('z'));
        assert_serialization!(Key::Char('A'));
        assert_serialization!(Key::Char('Z'));
        assert_serialization!(Key::Char('0'));
        assert_serialization!(Key::Char('9'));
        assert_serialization!(Key::Char('$'));
        assert_serialization!(Key::Ctrl('a'));
        assert_serialization!(Key::Ctrl('z'));
        assert_serialization!(Key::Ctrl('A'));
        assert_serialization!(Key::Ctrl('Z'));
        assert_serialization!(Key::Ctrl('0'));
        assert_serialization!(Key::Ctrl('9'));
        assert_serialization!(Key::Ctrl('$'));
        assert_serialization!(Key::Alt('a'));
        assert_serialization!(Key::Alt('z'));
        assert_serialization!(Key::Alt('A'));
        assert_serialization!(Key::Alt('Z'));
        assert_serialization!(Key::Alt('0'));
        assert_serialization!(Key::Alt('9'));
        assert_serialization!(Key::Alt('$'));
        assert_serialization!(Key::Esc);
    }
}
