use std::io;

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

pub type JsonInteger = i64;
pub type JsonNumber = f64;

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
        fn to_hex_digit(n: u8) -> u8 {
            if n <= 9 {
                n + b'0'
            } else {
                n - 10 + b'a'
            }
        }

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

pub struct JsonArray {
    first: u32,
    last: u32,
}

impl JsonArray {
    pub fn iter<'a>(&self, json: &'a Json) -> JsonElementIter<'a> {
        JsonElementIter {
            json,
            next: self.first as _,
        }
    }

    pub fn push(&mut self, json: &mut Json, value: JsonValue) {
        let index = json.elements.len() as u32;
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

pub struct JsonObject {
    first: u32,
    last: u32,
}

impl JsonObject {
    pub fn iter<'a>(&self, json: &'a Json) -> JsonMemberIter<'a> {
        JsonMemberIter {
            json,
            next: self.first as _,
        }
    }

    pub fn push(&mut self, json: &mut Json, key: &str, value: JsonValue) {
        let key = json.create_string(key);
        let index = json.members.len() as u32;
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

    pub fn parse(&mut self, json: &str) -> JsonValue {
        self.strings.clear();
        self.elements.truncate(1);
        self.members.truncate(1);

        JsonValue::Undefined
    }

    pub fn create_string(&mut self, value: &str) -> JsonString {
        let start = self.strings.len() as u32;
        self.strings.push_str(value);
        let end = self.strings.len() as u32;
        JsonString { start, end }
    }

    pub fn create_array(&mut self) -> JsonArray {
        JsonArray { first: 0, last: 0 }
    }

    pub fn create_object(&mut self) -> JsonObject {
        JsonObject { first: 0, last: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_elements() {
        let mut json = Json::new();
        let mut array = json.create_array();
        //array.push_element(&mut json, );
        //array.push_element(&mut json, json.create_bool(true));
    }
}
