use std::convert::TryFrom;

pub trait Serializer {
    fn write(&mut self, bytes: &[u8]);
}

pub struct DeserializeError;

pub trait Deserializer<'de> {
    fn read(&mut self, len: usize) -> Result<&'de [u8], DeserializeError>;
}

pub trait Serialize<'de>: Sized {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer;

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>;
}

macro_rules! impl_serialize_num {
    ($type:ty) => {
        impl<'de> Serialize<'de> for $type {
            fn serialize<S>(&self, serializer: &mut S)
            where
                S: Serializer,
            {
                serializer.write(&self.to_le_bytes());
            }

            fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
            where
                D: Deserializer<'de>,
            {
                let mut buf = [0; std::mem::size_of::<$type>()];
                let bytes = deserializer.read(buf.len())?;
                buf.clone_from_slice(bytes);
                Ok(<$type>::from_le_bytes(buf))
            }
        }
    };
}

impl_serialize_num!(u8);
impl_serialize_num!(u16);
impl_serialize_num!(u32);

impl<'de> Serialize<'de> for char {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        let value = *self as u32;
        value.serialize(serializer);
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        char::try_from(value).map_err(|_| DeserializeError)
    }
}

impl<'de> Serialize<'de> for &'de str {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        let len = self.len() as u32;
        len.serialize(serializer);
        serializer.write(self.as_bytes());
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        let bytes = deserializer.read(10)?;
        std::str::from_utf8(bytes).map_err(|_| DeserializeError)
    }
}

#[derive(Default)]
pub struct SerializationBuf(Vec<u8>);

impl SerializationBuf {
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl Serializer for SerializationBuf {
    fn write(&mut self, buf: &[u8]) {
        self.0.extend_from_slice(buf);
    }
}

pub struct DeserializationSlice<'de>(&'de [u8]);

impl<'de> DeserializationSlice<'de> {
    pub fn from_slice(slice: &'de [u8]) -> Self {
        Self(slice)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl<'de> Deserializer<'de> for DeserializationSlice<'de> {
    fn read(&mut self, len: usize) -> Result<&'de [u8], DeserializeError> {
        if len <= self.0.len() {
            let (before, after) = self.0.split_at(len);
            self.0 = after;
            Ok(before)
        } else {
            Err(DeserializeError)
        }
    }
}
