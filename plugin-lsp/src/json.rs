use std::{convert::From, fmt, io};

pub struct JsonConvertError;
pub trait FromJson<'json>: Sized {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError>;
}

macro_rules! impl_from_json {
    ($type:ty, $pattern:pat => $ok:expr) => {
        impl<'json> FromJson<'json> for $type {
            fn from_json(value: JsonValue, _: &'json Json) -> Result<Self, JsonConvertError> {
                match value {
                    $pattern => Ok($ok),
                    _ => Err(JsonConvertError),
                }
            }
        }
    };
}
impl_from_json!(bool, JsonValue::Boolean(b) => b);
impl_from_json!(JsonInteger, JsonValue::Integer(i) => i);
impl_from_json!(usize, JsonValue::Integer(i) => { if i < 0 { return Err(JsonConvertError); } i as _ });
impl_from_json!(u32, JsonValue::Integer(i) => { if i < 0 { return Err(JsonConvertError); } i as _ });
impl_from_json!(JsonNumber, JsonValue::Number(n) => n);
impl_from_json!(JsonString, JsonValue::String(s) => s);
impl_from_json!(JsonArray, JsonValue::Array(a) => a);
impl_from_json!(JsonObject, JsonValue::Object(o) => o);

impl<'json> FromJson<'json> for &'json str {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Str(s) => Ok(s),
            JsonValue::String(s) => Ok(s.as_str(json)),
            _ => Err(JsonConvertError),
        }
    }
}

impl<'json, T> FromJson<'json> for Option<T>
where
    T: FromJson<'json>,
{
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Null => Ok(None),
            _ => Ok(Some(T::from_json(value, json)?)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum JsonValue {
    Null,
    Boolean(bool),
    Integer(JsonInteger),
    Number(JsonNumber),
    Str(&'static str),
    String(JsonString),
    Array(JsonArray),
    Object(JsonObject),
}
impl JsonValue {
    pub fn get(self, key: &str, json: &Json) -> JsonValue {
        match self {
            JsonValue::Object(object) => object.get(key, json),
            _ => JsonValue::Null,
        }
    }

    pub fn elements(self, json: &Json) -> JsonElementIter {
        match self {
            JsonValue::Array(array) => array.elements(json),
            _ => JsonElementIter { json, next: 0 },
        }
    }

    pub fn members(self, json: &Json) -> JsonMemberIter {
        match self {
            JsonValue::Object(object) => object.members(json),
            _ => JsonMemberIter { json, next: 0 },
        }
    }
}
impl Default for JsonValue {
    fn default() -> Self {
        Self::Null
    }
}
impl<'json> FromJson<'json> for JsonValue {
    fn from_json(value: JsonValue, _: &'json Json) -> Result<Self, JsonConvertError> {
        Ok(value)
    }
}
impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}
impl From<JsonInteger> for JsonValue {
    fn from(value: JsonInteger) -> Self {
        Self::Integer(value)
    }
}
impl From<JsonNumber> for JsonValue {
    fn from(value: JsonNumber) -> Self {
        Self::Number(value)
    }
}
impl From<&'static str> for JsonValue {
    fn from(value: &'static str) -> Self {
        Self::Str(value)
    }
}
impl From<JsonString> for JsonValue {
    fn from(value: JsonString) -> Self {
        Self::String(value)
    }
}
impl From<JsonKey> for JsonValue {
    fn from(value: JsonKey) -> Self {
        match value {
            JsonKey::Str(s) => Self::Str(s),
            JsonKey::String(s) => Self::String(s),
        }
    }
}
impl From<JsonArray> for JsonValue {
    fn from(value: JsonArray) -> Self {
        Self::Array(value)
    }
}
impl From<JsonObject> for JsonValue {
    fn from(value: JsonObject) -> Self {
        Self::Object(value)
    }
}

pub type JsonInteger = i64;
pub type JsonNumber = f64;

#[derive(Debug, Default, Clone)]
pub struct JsonString {
    start: u32,
    end: u32,
}
impl JsonString {
    pub fn as_str<'a>(&self, json: &'a Json) -> &'a str {
        &json.strings[self.start as usize..self.end as usize]
    }
}

#[derive(Debug, Default, Clone)]
pub struct JsonArray {
    first: u32,
    last: u32,
}
impl JsonArray {
    pub fn elements(self, json: &Json) -> JsonElementIter {
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
}

pub enum JsonKey {
    Str(&'static str),
    String(JsonString),
}
impl JsonKey {
    pub fn as_str<'a>(&self, json: &'a Json) -> &'a str {
        match self {
            JsonKey::Str(s) => s,
            JsonKey::String(s) => s.as_str(json),
        }
    }
}
impl Default for JsonKey {
    fn default() -> Self {
        Self::String(JsonString::default())
    }
}
impl<'json> FromJson<'json> for JsonKey {
    fn from_json(value: JsonValue, _: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Str(s) => Ok(JsonKey::Str(s)),
            JsonValue::String(s) => Ok(JsonKey::String(s)),
            _ => Err(JsonConvertError),
        }
    }
}
impl From<&'static str> for JsonKey {
    fn from(value: &'static str) -> Self {
        Self::Str(value)
    }
}
impl From<JsonString> for JsonKey {
    fn from(value: JsonString) -> Self {
        Self::String(value)
    }
}

#[derive(Debug, Default, Clone)]
pub struct JsonObject {
    first: u32,
    last: u32,
}
impl JsonObject {
    pub fn get(self, key: &str, json: &Json) -> JsonValue {
        for (k, v) in self.members(json) {
            if k == key {
                return v;
            }
        }
        JsonValue::Null
    }

    pub fn members(self, json: &Json) -> JsonMemberIter {
        JsonMemberIter {
            json,
            next: self.first as _,
        }
    }

    pub fn set(&mut self, key: JsonKey, value: JsonValue, json: &mut Json) {
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
}

struct JsonArrayElement {
    value: JsonValue,
    next: u32,
}

struct JsonObjectMember {
    key: JsonKey,
    value: JsonValue,
    next: u32,
}

pub struct JsonElementIter<'a> {
    json: &'a Json,
    next: usize,
}

impl<'a> Iterator for JsonElementIter<'a> {
    type Item = JsonValue;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next != 0 {
            let element = &self.json.elements[self.next];
            self.next = element.next as usize;
            Some(element.value.clone())
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
    type Item = (&'a str, JsonValue);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next != 0 {
            let member = &self.json.members[self.next];
            self.next = member.next as usize;
            Some((member.key.as_str(self.json), member.value.clone()))
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
                value: JsonValue::Null,
                next: 0,
            }],
            members: vec![JsonObjectMember {
                key: JsonKey::Str(""),
                value: JsonValue::Null,
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

    pub fn fmt_string(&mut self, args: fmt::Arguments) -> JsonString {
        let start = self.strings.len() as _;
        let _ = fmt::write(&mut self.strings, args);
        let end = self.strings.len() as _;
        JsonString { start, end }
    }

    pub fn clear(&mut self) {
        self.strings.clear();
        self.elements.truncate(1);
        self.members.truncate(1);
    }

    pub fn read(&mut self, reader: &mut dyn io::BufRead) -> io::Result<JsonValue> {
        fn next_byte(reader: &mut dyn io::BufRead) -> io::Result<u8> {
            let mut buf = [0; 1];
            if reader.read(&mut buf)? == buf.len() {
                Ok(buf[0])
            } else {
                Err(io::Error::from(io::ErrorKind::UnexpectedEof))
            }
        }

        fn match_byte(reader: &mut dyn io::BufRead, byte: u8) -> io::Result<bool> {
            let buf = reader.fill_buf()?;
            if !buf.is_empty() && buf[0] == byte {
                reader.consume(1);
                Ok(true)
            } else {
                Ok(false)
            }
        }

        fn skip_whitespace(reader: &mut dyn io::BufRead) -> io::Result<()> {
            loop {
                let buf = reader.fill_buf()?;
                match buf
                    .iter()
                    .enumerate()
                    .find(|(_, c)| !c.is_ascii_whitespace())
                {
                    Some((0, _)) | None => return Ok(()),
                    Some((i, _)) => reader.consume(i),
                }
            }
        }

        fn consume_bytes<const LEN: usize>(
            reader: &mut dyn io::BufRead,
            bytes: &[u8; LEN],
        ) -> io::Result<()> {
            let mut buf = [0; LEN];
            if reader.read(&mut buf)? == buf.len() {
                if &buf == bytes {
                    Ok(())
                } else {
                    Err(invalid_data_error())
                }
            } else {
                Err(io::Error::from(io::ErrorKind::UnexpectedEof))
            }
        }

        fn consume_string(json: &mut Json, reader: &mut dyn io::BufRead) -> io::Result<JsonString> {
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

                                let mut c: u32 = 0;
                                c += from_hex_digit(buf[0])? << 12;
                                c += from_hex_digit(buf[1])? << 8;
                                c += from_hex_digit(buf[2])? << 4;
                                c += from_hex_digit(buf[3])?;
                                c = u32::from_le(c);

                                match std::char::from_u32(c) {
                                    Some(c) => json.strings.push(c),
                                    None => return Err(invalid_data_error()),
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

        fn read_value(json: &mut Json, reader: &mut dyn io::BufRead) -> io::Result<JsonValue> {
            skip_whitespace(reader)?;
            match next_byte(reader)? {
                b'n' => {
                    consume_bytes(reader, b"ull")?;
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Null)
                }
                b'f' => {
                    consume_bytes(reader, b"alse")?;
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Boolean(false))
                }
                b't' => {
                    consume_bytes(reader, b"rue")?;
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Boolean(true))
                }
                b'"' => consume_string(json, reader).map(JsonValue::String),
                b'[' => {
                    skip_whitespace(reader)?;
                    let mut array = JsonArray::default();
                    if !match_byte(reader, b']')? {
                        loop {
                            array.push(read_value(json, reader)?, json);
                            if match_byte(reader, b']')? {
                                break;
                            }
                            consume_bytes(reader, b",")?;
                        }
                    }
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Array(array))
                }
                b'{' => {
                    skip_whitespace(reader)?;
                    let mut object = JsonObject::default();
                    if !match_byte(reader, b'}')? {
                        loop {
                            skip_whitespace(reader)?;
                            consume_bytes(reader, b"\"")?;
                            let key = consume_string(json, reader)?;
                            consume_bytes(reader, b":")?;
                            object.set(JsonKey::String(key), read_value(json, reader)?, json);
                            if match_byte(reader, b'}')? {
                                break;
                            }
                            consume_bytes(reader, b",")?;
                        }
                    }
                    skip_whitespace(reader)?;
                    Ok(JsonValue::Object(object))
                }
                b => {
                    fn next_digit(reader: &mut dyn io::BufRead) -> io::Result<Option<u8>> {
                        let buf = reader.fill_buf()?;
                        if !buf.is_empty() {
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

    pub fn write(&self, buf: &mut dyn io::Write, value: &JsonValue) -> io::Result<()> {
        fn append_str(buf: &mut dyn io::Write, s: &str) -> io::Result<()> {
            buf.write_all(b"\"")?;
            for c in s.chars() {
                match c {
                    '\"' => buf.write_all(b"\\\"")?,
                    '\\' => buf.write_all(b"\\\\")?,
                    '\x08' => buf.write_all(b"\\b")?,
                    '\x0c' => buf.write_all(b"\\f")?,
                    '\n' => buf.write_all(b"\\n")?,
                    '\r' => buf.write_all(b"\\r")?,
                    '\t' => buf.write_all(b"\\t")?,
                    _ => {
                        let c = c as u32;
                        if c >= 32 && c <= 126 {
                            buf.write_all(&[c as _])?;
                        } else {
                            fn to_hex_digit(n: u32) -> u8 {
                                let n = (n & 0xf) as u8;
                                if n <= 9 {
                                    n + b'0'
                                } else {
                                    n - 10 + b'a'
                                }
                            }

                            buf.write_all(b"\\u")?;
                            let c = c.to_le();
                            buf.write_all(&[
                                to_hex_digit(c >> 12),
                                to_hex_digit(c >> 8),
                                to_hex_digit(c >> 4),
                                to_hex_digit(c),
                            ])?;
                        }
                    }
                }
            }
            buf.write_all(b"\"")?;
            Ok(())
        }

        match value {
            JsonValue::Null => buf.write_all(b"null"),
            JsonValue::Boolean(true) => buf.write_all(b"true"),
            JsonValue::Boolean(false) => buf.write_all(b"false"),
            JsonValue::Integer(i) => {
                write!(buf, "{}", i)
            }
            JsonValue::Number(n) => {
                write!(buf, "{}", n)
            }
            JsonValue::Str(s) => append_str(buf, s),
            JsonValue::String(s) => append_str(buf, s.as_str(self)),
            JsonValue::Array(a) => {
                buf.write_all(b"[")?;
                let mut next = a.first as usize;
                if next != 0 {
                    loop {
                        let element = &self.elements[next];
                        self.write(buf, &element.value)?;
                        next = element.next as _;
                        if next == 0 {
                            break;
                        }
                        buf.write_all(b",")?;
                    }
                }
                buf.write_all(b"]")?;
                Ok(())
            }
            JsonValue::Object(o) => {
                buf.write_all(b"{")?;
                let mut next = o.first as usize;
                if next != 0 {
                    loop {
                        let member = &self.members[next];
                        append_str(buf, member.key.as_str(self))?;
                        buf.write_all(b":")?;
                        self.write(buf, &member.value)?;
                        next = member.next as _;
                        if next == 0 {
                            break;
                        }
                        buf.write_all(b",")?;
                    }
                }
                buf.write_all(b"}")?;
                Ok(())
            }
        }
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
    fn write_value() {
        fn assert_json(buf: &mut Vec<u8>, expected: &str, value: JsonValue, json: &mut Json) {
            buf.clear();
            json.write(buf, &value).unwrap();
            assert_eq!(expected, std::str::from_utf8(buf).unwrap());
        }

        let mut json = Json::new();
        let mut buf = Vec::new();

        assert_json(&mut buf, "null", JsonValue::Null, &mut json);
        assert_json(&mut buf, "false", JsonValue::Boolean(false), &mut json);
        assert_json(&mut buf, "true", JsonValue::Boolean(true), &mut json);
        assert_json(&mut buf, "0", JsonValue::Integer(0), &mut json);
        assert_json(&mut buf, "1", JsonValue::Integer(1), &mut json);
        assert_json(&mut buf, "-1", JsonValue::Integer(-1), &mut json);
        assert_json(&mut buf, "0.5", JsonValue::Number(0.5), &mut json);
        assert_json(
            &mut buf,
            "\"string\"",
            json.create_string("string").into(),
            &mut json,
        );
        assert_json(
            &mut buf,
            "\"\\u00e1\"",
            json.create_string("\u{00e1}").into(),
            &mut json,
        );
        assert_json(
            &mut buf,
            "\"\\ufa09\"",
            json.create_string("\u{fa09}").into(),
            &mut json,
        );
        assert_json(
            &mut buf,
            "\"\\\"\\\\/\\b\\f\\n\\r\\t\"",
            json.create_string("\"\\/\x08\x0c\n\r\t").into(),
            &mut json,
        );
    }

    #[test]
    fn write_complex() {
        let mut json = Json::new();
        let mut array = JsonArray::default();

        array.push(JsonValue::Boolean(true), &mut json);
        array.push(JsonValue::Integer(8), &mut json);
        array.push(JsonValue::Number(0.5), &mut json);
        array.push(json.create_string("text").into(), &mut json);

        let mut object = JsonObject::default();
        object.set("first".into(), JsonValue::Null, &mut json);
        object.set("second".into(), "txt".into(), &mut json);

        array.push(object.into(), &mut json);
        array.push(JsonArray::default().into(), &mut json);
        array.push(JsonObject::default().into(), &mut json);

        let mut buf = Vec::new();
        let array = array.into();
        json.write(&mut buf, &array).unwrap();
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
                json.clear();
                let value = json.read(&mut reader).unwrap();
                assert!(matches!(value, $expected), "got {:?}", value);
            };
            ($expected:pat, $text:expr => $and_then:expr) => {
                let mut reader = Cursor::new($text.as_bytes());
                json.clear();
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
        assert_json!(JsonValue::String(s), "\"\\u00e1\"" => assert_eq!("\u{00e1}", s.as_str(&json)));
        assert_json!(JsonValue::String(s), "\"\\ufa09\"" => assert_eq!("\u{fa09}", s.as_str(&json)));
        assert_json!(JsonValue::String(s), "\"\\\"\\\\\\/\\b\\f\\n\\r\\t\"" => assert_eq!("\"\\/\x08\x0c\n\r\t", s.as_str(&json)));
        assert_json!(
            JsonValue::String(s),
            "\"file:///c:/path/to/file.ext\"" =>
            assert_eq!("file:///c:/path/to/file.ext", s.as_str(&json))
        );
    }

    #[test]
    fn read_complex() {
        let mut json = Json::new();
        let mut reader = Cursor::new(b" { \"array\"  : [\"string\",  false,null,  0.25,\n{\"int\":  7,  \"bool\":false,\"null\":null, \t\n   \"string\":\"some text\"},[]],   \n\"str\":\"asdad\", \"empty\":{}}   ");
        let value = json.read(&mut reader).unwrap();

        macro_rules! assert_next {
            ($iter:expr, $pattern:pat => $and_then:expr) => {
                match $iter.next() {
                    Some($pattern) => $and_then,
                    v => assert!(false, "got {:?}", v),
                }
            };
        }

        match value {
            JsonValue::Object(o) => {
                let mut members = o.members(&json);

                assert_next!(members, ("array", JsonValue::Array(a)) => {
                    let mut elements = a.elements(&json);

                    assert_next!(elements, JsonValue::String(s) => assert_eq!("string", s.as_str(&json)));
                    assert_next!(elements, JsonValue::Boolean(false) => assert!(true));
                    assert_next!(elements, JsonValue::Null => assert!(true));
                    assert_next!(elements, JsonValue::Number(n) => assert_eq!(0.25, n));

                    assert_next!(elements, JsonValue::Object(o) => {
                        let mut members = o.members(&json);

                        assert_next!(members, ("int", JsonValue::Integer(7)) => assert!(true));
                        assert_next!(members, ("bool", JsonValue::Boolean(false)) => assert!(true));
                        assert_next!(members, ("null", JsonValue::Null) => assert!(true));
                        assert_next!(members, ("string", JsonValue::String(s)) => assert_eq!("some text", s.as_str(&json)));
                    });

                    assert_next!(elements, JsonValue::Array(a) => {
                        assert!(a.elements(&json).next().is_none());
                    });

                    assert!(elements.next().is_none());
                });

                assert_next!(members, ("str", JsonValue::String(s)) => assert_eq!("asdad", s.as_str(&json)));
                assert_next!(members, ("empty", JsonValue::Object(o)) => {
                    assert!(o.members(&json).next().is_none());
                });
            }
            _ => assert!(false, "got {:?}", value),
        }
    }
}

