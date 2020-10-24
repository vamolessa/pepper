use std::io;

pub struct JsonValue<'a> {
    json: &'a Json,
    inner: JsonValueImpl,
}

impl<'a> JsonValue<'a> {
    pub fn to_bool(&self) -> Option<bool> {
        match self.inner {
            JsonValueImpl::Boolean(b) => Some(b),
            _ => None,
        }
    }

    pub fn to_integer(&self) -> Option<i64> {
        match self.inner {
            JsonValueImpl::Integer(b) => Some(b),
            _ => None,
        }
    }

    pub fn to_number(&self) -> Option<f64> {
        match self.inner {
            JsonValueImpl::Number(b) => Some(b),
            _ => None,
        }
    }

    pub fn to_string(&self) -> Option<&'a str> {
        match self.inner {
            JsonValueImpl::String(s) => Some(self.json.get_string(s)),
            _ => None,
        }
    }

    pub fn elements(&self) -> JsonArray<'a> {
        let next = match self.inner {
            JsonValueImpl::Array(i) => i,
            _ => 0,
        };

        JsonArray {
            json: self.json,
            next,
        }
    }

    pub fn members(&self) -> JsonObject<'a> {
        let next = match self.inner {
            JsonValueImpl::Object(i) => i,
            _ => 0,
        };

        JsonObject {
            json: self.json,
            next,
        }
    }

    pub fn write<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        match self.inner {
            JsonValueImpl::Undefined | JsonValueImpl::Null => {
                writer.write(b"null")?;
            }
            JsonValueImpl::Boolean(true) => {
                writer.write(b"true")?;
            }
            JsonValueImpl::Boolean(false) => {
                writer.write(b"false")?;
            }
            JsonValueImpl::Integer(i) => writer.write_fmt(format_args!("{}", i))?,
            JsonValueImpl::Number(n) => writer.write_fmt(format_args!("{}", n))?,
            JsonValueImpl::String(r) => write_str(writer, self.json.get_string(r))?,
            _ => (),
        }
        Ok(())
    }
}

pub struct JsonArray<'a> {
    json: &'a Json,
    next: usize,
}

impl<'a> Iterator for JsonArray<'a> {
    type Item = JsonValue<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let element = self.json.elements[self.next];
        self.next = element.next;
        match element.value {
            JsonValueImpl::Undefined => None,
            _ => Some(JsonValue {
                json: self.json,
                inner: element.value,
            }),
        }
    }
}

pub struct JsonObject<'a> {
    json: &'a Json,
    next: usize,
}

impl<'a> Iterator for JsonObject<'a> {
    type Item = (&'a str, JsonValue<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let member = self.json.members[self.next];
        self.next = member.next;
        match member.value {
            JsonValueImpl::Undefined => None,
            _ => Some((
                self.json.get_string(member.key),
                JsonValue {
                    json: self.json,
                    inner: member.value,
                },
            )),
        }
    }
}

#[derive(Clone, Copy)]
enum JsonValueImpl {
    Undefined,
    Null,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(JsonStringRange),
    Array(usize),
    Object(usize),
}

#[derive(Clone, Copy)]
struct JsonStringRange {
    from: usize,
    to: usize,
}

#[derive(Clone, Copy)]
struct JsonArrayElement {
    value: JsonValueImpl,
    next: usize,
}

#[derive(Clone, Copy)]
struct JsonObjectMember {
    key: JsonStringRange,
    value: JsonValueImpl,
    next: usize,
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
                value: JsonValueImpl::Undefined,
                next: 0,
            }],
            members: vec![JsonObjectMember {
                key: JsonStringRange { from: 0, to: 0 },
                value: JsonValueImpl::Undefined,
                next: 0,
            }],
        }
    }

    pub fn parse<'a>(&'a mut self, json: &'a str) -> JsonValue<'a> {
        self.strings.clear();
        self.elements.truncate(1);
        self.members.truncate(1);

        JsonValue {
            json: self,
            inner: JsonValueImpl::Undefined,
        }
    }

    fn get_string<'a>(&'a self, range: JsonStringRange) -> &'a str {
        &self.strings[range.from..range.to]
    }
}

fn write_str<W>(writer: &mut W, s: &str) -> io::Result<()>
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
    for c in s.chars() {
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

#[cfg(test)]
mod tests {
    use super::*;
}
