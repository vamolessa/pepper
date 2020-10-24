use std::{convert::From, io};

#[derive(Debug)]
pub enum JsonValue {
    Undefined,
    Null,
    Boolean(bool),
    Integer(JsonInteger),
    Number(JsonNumber),
    String(JsonString),
    Array(JsonArray),
    Object(JsonObject),
}

impl JsonValue {
    pub fn write<W>(&self, json: &Json, writer: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        match self {
            JsonValue::Undefined | JsonValue::Null => {
                writer.write(b"null")?;
            }
            JsonValue::Boolean(true) => {
                writer.write(b"true")?;
            }
            JsonValue::Boolean(false) => {
                writer.write(b"false")?;
            }
            JsonValue::Integer(i) => writer.write_fmt(format_args!("{}", i))?,
            JsonValue::Number(n) => writer.write_fmt(format_args!("{}", n))?,
            JsonValue::String(s) => s.write(json, writer)?,
            JsonValue::Array(a) => a.write(json, writer)?,
            JsonValue::Object(o) => o.write(json, writer)?,
        }
        Ok(())
    }
}

impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        JsonValue::Boolean(value)
    }
}
impl From<JsonInteger> for JsonValue {
    fn from(value: JsonInteger) -> Self {
        JsonValue::Integer(value)
    }
}
impl From<JsonNumber> for JsonValue {
    fn from(value: JsonNumber) -> Self {
        JsonValue::Number(value)
    }
}
impl From<JsonString> for JsonValue {
    fn from(value: JsonString) -> Self {
        JsonValue::String(value)
    }
}
impl From<JsonArray> for JsonValue {
    fn from(value: JsonArray) -> Self {
        JsonValue::Array(value)
    }
}
impl From<JsonObject> for JsonValue {
    fn from(value: JsonObject) -> Self {
        JsonValue::Object(value)
    }
}

pub type JsonInteger = i64;
pub type JsonNumber = f64;

#[derive(Debug)]
pub struct JsonString {
    start: u32,
    end: u32,
}

impl JsonString {
    pub fn as_str<'a>(&self, json: &'a Json) -> &'a str {
        &json.strings[(self.start as usize)..(self.end as usize)]
    }

    pub fn write<W>(&self, json: &Json, writer: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        writer.write(b"\"")?;
        for c in self.as_str(json).chars() {
            let _ = match c {
                '\"' => writer.write(b"\\\"")?,
                '\\' => writer.write(b"\\\\")?,
                '\x08' => writer.write(b"\\b")?,
                '\x0c' => writer.write(b"\\f")?,
                '\n' => writer.write(b"\\n")?,
                '\r' => writer.write(b"\\r")?,
                '\t' => writer.write(b"\\t")?,
                _ => {
                    let c = c as u32;
                    if c >= 32 && c <= 126 {
                        writer.write(&[c as u8])?;
                    } else {
                        fn to_hex_digit(n: u8) -> u8 {
                            if n <= 9 {
                                n + b'0'
                            } else {
                                n - 10 + b'a'
                            }
                        }

                        writer.write(b"\\u")?;
                        let bytes = c.to_le_bytes();
                        writer.write(&[
                            to_hex_digit(bytes[3]),
                            to_hex_digit(bytes[2]),
                            to_hex_digit(bytes[1]),
                            to_hex_digit(bytes[0]),
                        ])?;
                    }
                    0
                }
            };
        }
        writer.write(b"\"")?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct JsonArray {
    first: u32,
    last: u32,
}

impl JsonArray {
    pub fn new() -> Self {
        Self { first: 0, last: 0 }
    }

    pub fn iter<'a>(&self, json: &'a Json) -> JsonElementIter<'a> {
        JsonElementIter {
            json,
            next: self.first as _,
        }
    }

    pub fn push(&mut self, value: JsonValue, json: &mut Json) {
        let index = json.elements.len() as _;
        json.elements.push(JsonArrayElement { value, next: 0 });
        if self.first != 0 {
            json.elements[self.last as usize].next = index;
        } else {
            self.first = index;
        }
        self.last = index;
    }

    pub fn write<W>(&self, json: &Json, writer: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        writer.write(b"[")?;
        let mut next = self.first as usize;
        if next != 0 {
            loop {
                let element = &json.elements[next];
                element.value.write(json, writer)?;
                next = element.next as _;
                if next == 0 {
                    break;
                }
                writer.write(b",")?;
            }
        }
        writer.write(b"]")?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct JsonObject {
    first: u32,
    last: u32,
}

impl JsonObject {
    pub fn new() -> Self {
        Self { first: 0, last: 0 }
    }

    pub fn iter<'a>(&self, json: &'a Json) -> JsonMemberIter<'a> {
        JsonMemberIter {
            json,
            next: self.first as _,
        }
    }

    pub fn push(&mut self, key: JsonString, value: JsonValue, json: &mut Json) {
        let index = json.members.len() as _;
        json.members.push(JsonObjectMember {
            key,
            value,
            next: 0,
        });
        if self.first != 0 {
            json.members[self.last as usize].next = index;
        } else {
            self.first = index;
        }
        self.last = index;
    }

    pub fn write<W>(&self, json: &Json, writer: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        writer.write(b"{")?;
        let mut next = self.first as usize;
        if next != 0 {
            loop {
                let member = &json.members[next];
                member.key.write(json, writer)?;
                writer.write(b":")?;
                member.value.write(json, writer)?;
                next = member.next as _;
                if next == 0 {
                    break;
                }
                writer.write(b",")?;
            }
        }
        writer.write(b"}")?;
        Ok(())
    }
}

struct JsonArrayElement {
    value: JsonValue,
    next: u32,
}

struct JsonObjectMember {
    key: JsonString,
    value: JsonValue,
    next: u32,
}

pub struct JsonElementIter<'a> {
    json: &'a Json,
    next: usize,
}

impl<'a> Iterator for JsonElementIter<'a> {
    type Item = &'a JsonValue;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next != 0 {
            let element = &self.json.elements[self.next];
            self.next = element.next as usize;
            Some(&element.value)
        } else {
            None
        }
    }
}

pub struct JsonMemberIter<'a> {
    json: &'a Json,
    next: usize,
}

impl<'a> Iterator for JsonMemberIter<'a> {
    type Item = (&'a str, &'a JsonValue);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next != 0 {
            let member = &self.json.members[self.next];
            self.next = member.next as usize;
            Some((member.key.as_str(self.json), &member.value))
        } else {
            None
        }
    }
}

pub struct Json {
    strings: String,
    elements: Vec<JsonArrayElement>,
    members: Vec<JsonObjectMember>,
}

impl Json {
    pub fn new() -> Self {
        Self {
            strings: String::new(),
            elements: vec![JsonArrayElement {
                value: JsonValue::Undefined,
                next: 0,
            }],
            members: vec![JsonObjectMember {
                key: JsonString { start: 0, end: 0 },
                value: JsonValue::Undefined,
                next: 0,
            }],
        }
    }

    pub fn create_string(&mut self, value: &str) -> JsonString {
        let start = self.strings.len() as _;
        self.strings.push_str(value);
        let end = self.strings.len() as _;
        JsonString { start, end }
    }

    pub fn read<R>(&mut self, reader: &mut R) -> io::Result<JsonValue>
    where
        R: io::BufRead,
    {
        self.strings.clear();
        self.elements.truncate(1);
        self.members.truncate(1);

        fn next_byte<R>(reader: &mut R) -> io::Result<u8>
        where
            R: io::BufRead,
        {
            let mut buf = [0; 1];
            if reader.read(&mut buf)? == buf.len() {
                Ok(buf[0])
            } else {
                Err(io::Error::from(io::ErrorKind::UnexpectedEof))
            }
        }

        fn match_byte<R>(reader: &mut R, byte: u8) -> io::Result<bool>
        where
            R: io::BufRead,
        {
            let buf = reader.fill_buf()?;
            if buf.len() > 0 && buf[0] == byte {
                reader.consume(1);
                Ok(true)
            } else {
                Ok(false)
            }
        }

        fn skip_whitespace<R>(reader: &mut R) -> io::Result<()>
        where
            R: io::BufRead,
        {
            loop {
                let buf = reader.fill_buf()?;
                match buf.iter().position(u8::is_ascii_whitespace) {
                    Some(i) => reader.consume(i),
                    None => break Ok(()),
                }
            }
        }

        macro_rules! consume_bytes {
            ($reader:expr, $bytes:expr) => {{
                let mut buf = [0; $bytes.len()];
                if $reader.read(&mut buf)? == buf.len() {
                    if &buf != $bytes {
                        return Err(invalid_data_error());
                    }
                } else {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                }
            }};
        }

        fn consume_string<R>(json: &mut Json, reader: &mut R) -> io::Result<JsonString>
        where
            R: io::BufRead,
        {
            let start = json.strings.len();
            loop {
                match next_byte(reader)? {
                    b'"' => {
                        skip_whitespace(reader)?;
                        return Ok(JsonString {
                            start: start as _,
                            end: json.strings.len() as _,
                        });
                    }
                    b'\\' => match next_byte(reader)? {
                        b'"' => json.strings.push('"'),
                        b'\\' => json.strings.push('\\'),
                        b'/' => json.strings.push('/'),
                        b'b' => json.strings.push('\x08'),
                        b'f' => json.strings.push('\x0c'),
                        b'n' => json.strings.push('\n'),
                        b'r' => json.strings.push('\r'),
                        b't' => json.strings.push('\t'),
                        b'u' => {
                            let mut buf = [0; 4];
                            if reader.read(&mut buf)? == buf.len() {
                                fn from_hex_digit(d: u8) -> io::Result<u32> {
                                    match d {
                                        b'0'..=b'9' => Ok((d - b'0') as _),
                                        b'a'..=b'f' => Ok((10 + d - b'a') as _),
                                        b'A'..=b'F' => Ok((10 + d - b'A') as _),
                                        _ => Err(invalid_data_error()),
                                    }
                                }

                                let mut c : u32 = 0;
                                c += from_hex_digit(buf[0])? << 12;
                                c += from_hex_digit(buf[1])? << 8;
                                c += from_hex_digit(buf[2])? << 4;
                                c += from_hex_digit(buf[3])?;

                                match std::char::from_u32(c) {
                                    Some(c) => json.strings.push(c),
                                    None => {
                                        dbg!(c, '\u{fa09}' as u32);
                                        return Err(invalid_data_error());
                                    }
                                }
                            } else {
                                return Err(invalid_data_error());
                            }
                        }
                        _ => return Err(invalid_data_error()),
                    },
                    c => json.strings.push(c as _),
                }
            }
        }

        fn read_value<R>(json: &mut Json, reader: &mut R) -> io::Result<JsonValue>
        where
            R: io::BufRead,
        {
            skip_whitespace(reader)?;
            match next_byte(reader)? {
                b'n' => {
                    consume_bytes!(reader, b"ull");
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Null)
                }
                b'f' => {
                    consume_bytes!(reader, b"alse");
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Boolean(false))
                }
                b't' => {
                    consume_bytes!(reader, b"rue");
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Boolean(true))
                }
                b'"' => consume_string(json, reader).map(JsonValue::String),
                b'[' => {
                    skip_whitespace(reader)?;
                    let mut array = JsonArray::new();
                    if !match_byte(reader, b']')? {
                        loop {
                            array.push(read_value(json, reader)?, json);
                            if match_byte(reader, b']')? {
                                break;
                            }
                        }
                    }
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Array(array))
                }
                b'{' => {
                    skip_whitespace(reader)?;
                    let mut object = JsonObject::new();
                    if !match_byte(reader, b'}')? {
                        loop {
                            skip_whitespace(reader)?;
                            consume_bytes!(reader, b"\"");
                            let key = consume_string(json, reader)?;
                            consume_bytes!(reader, b":");
                            object.push(key, read_value(json, reader)?, json);
                            if match_byte(reader, b'}')? {
                                break;
                            }
                        }
                    }
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Object(object))
                }
                b => {
                    fn next_digit<R>(reader: &mut R) -> io::Result<Option<u8>>
                    where
                        R: io::BufRead,
                    {
                        let buf = reader.fill_buf()?;
                        if buf.len() > 0 {
                            let byte = buf[0];
                            if byte.is_ascii_digit() {
                                reader.consume(1);
                                Ok(Some(byte - b'0'))
                            } else {
                                Ok(None)
                            }
                        } else {
                            Ok(None)
                        }
                    }

                    let mut integer: JsonInteger = 0;

                    let is_negative = b == b'-';
                    if !is_negative {
                        if b.is_ascii_digit() {
                            integer = (b - b'0') as _;
                        } else {
                            return Err(invalid_data_error());
                        }
                    }

                    if integer == 0 {
                        while match_byte(reader, b'0')? {}
                    }

                    while let Some(d) = next_digit(reader)? {
                        match integer.checked_mul(10).and_then(|n| n.checked_add(d as _)) {
                            Some(n) => integer = n,
                            None => return Err(invalid_data_error()),
                        }
                    }

                    if match_byte(reader, b'.')? {
                        let mut fraction_base: JsonNumber = 1.0;
                        let mut fraction: JsonNumber = 0.0;

                        while let Some(d) = next_digit(reader)? {
                            fraction_base *= 0.1;
                            fraction += (d as JsonNumber) * fraction_base;
                        }

                        fraction += integer as JsonNumber;
                        if is_negative {
                            fraction = -fraction;
                        }

                        skip_whitespace(reader)?;
                        Ok(JsonValue::Number(fraction))
                    } else {
                        if is_negative {
                            integer = -integer;
                        }

                        skip_whitespace(reader)?;
                        Ok(JsonValue::Integer(integer))
                    }
                }
            }
        }

        read_value(self, reader)
    }
}

fn invalid_data_error() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Cursor;

    #[test]
    fn write_complex() {
        let mut json = Json::new();
        let mut array = JsonArray::new();

        array.push(JsonValue::Boolean(true), &mut json);
        array.push(JsonValue::Integer(8), &mut json);
        array.push(JsonValue::Number(0.5), &mut json);
        array.push(json.create_string("text").into(), &mut json);

        let mut object = JsonObject::new();
        object.push(json.create_string("first"), JsonValue::Null, &mut json);
        object.push(
            json.create_string("second"),
            json.create_string("txt").into(),
            &mut json,
        );

        array.push(object.into(), &mut json);
        array.push(JsonArray::new().into(), &mut json);
        array.push(JsonObject::new().into(), &mut json);

        let mut buf = Vec::new();
        array.write(&json, &mut buf).unwrap();
        let json = String::from_utf8(buf).unwrap();
        assert_eq!(
            "[true,8,0.5,\"text\",{\"first\":null,\"second\":\"txt\"},[],{}]",
            json
        );
    }

    #[test]
    fn read_value() {
        let mut json = Json::new();

        macro_rules! assert_json {
            ($expected:pat, $text:expr) => {
                let mut reader = Cursor::new($text.as_bytes());
                let value = json.read(&mut reader).unwrap();
                assert!(matches!(value, $expected), "got {:?}", value);
            };
            ($expected:pat, $text:expr => $and_then:expr) => {
                let mut reader = Cursor::new($text.as_bytes());
                let value = json.read(&mut reader).unwrap();
                match value {
                    $expected => $and_then,
                    _ => assert!(false, "got {:?}", value),
                }
            };
        }

        assert_json!(JsonValue::Null, "null");
        assert_json!(JsonValue::Boolean(false), "false");
        assert_json!(JsonValue::Boolean(true), "true");
        assert_json!(JsonValue::Integer(0), "0");
        assert_json!(JsonValue::Integer(0), "000");
        assert_json!(JsonValue::Integer(-1), "-001");
        assert_json!(JsonValue::Number(n), "0.5" => assert_eq!(0.5, n));
        assert_json!(JsonValue::String(s), "\"string\"" => assert_eq!("string", s.as_str(&json)));
        assert_json!(JsonValue::String(s), "\"\\ufa09\"" => assert_eq!("\u{fa09}", s.as_str(&json)));
    }
}
