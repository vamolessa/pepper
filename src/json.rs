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

    pub fn elements(&self) -> Option<JsonElementIter<'a>> {
        match self.inner {
            JsonValueImpl::Array(r) => Some(JsonElementIter {
                json: self.json,
                next: r.from as _,
            }),
            _ => None,
        }
    }

    pub fn push_element(&mut self, json: &'a mut Json, element: JsonValue<'a>) {
        if let JsonValueImpl::Array(r) = self.inner {
            let index = json.elements.len() as u32;
            json.elements.push(JsonArrayElement {
                value: element.inner,
                next: index,
            });
            if r.from == r.to {
                *self = JsonValue {
                    json,
                    inner: JsonValueImpl::Array(JsonRange {
                        from: index,
                        to: index,
                    }),
                };
            } else {
                json.elements[r.to as usize].next = index;
                *self = JsonValue {
                    json,
                    inner: JsonValueImpl::Array(JsonRange {
                        from: r.from,
                        to: index,
                    }),
                };
            }
        }
    }

    pub fn members(&self) -> Option<JsonMemberIter<'a>> {
        match self.inner {
            JsonValueImpl::Object(r) => Some(JsonMemberIter {
                json: self.json,
                next: r.from as _,
            }),
            _ => None,
        }
    }

    pub fn push_member(&mut self, json: &'a mut Json, key: &str, member: JsonValue<'a>) {
        if let JsonValueImpl::Object(r) = self.inner {
            let key = json.create_string_range(key);
            let index = json.members.len() as u32;
            json.members.push(JsonObjectMember {
                key,
                value: member.inner,
                next: index,
            });
            if r.from == r.to {
                *self = JsonValue {
                    json,
                    inner: JsonValueImpl::Object(JsonRange {
                        from: index,
                        to: index,
                    }),
                };
            } else {
                json.members[r.to as usize].next = index;
                *self = JsonValue {
                    json,
                    inner: JsonValueImpl::Object(JsonRange {
                        from: r.from,
                        to: index,
                    }),
                };
            }
        }
    }

    pub fn write<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        write_value(self.json, writer, self.inner)
    }
}

pub struct JsonElementIter<'a> {
    json: &'a Json,
    next: usize,
}

impl<'a> Iterator for JsonElementIter<'a> {
    type Item = JsonValue<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let element = self.json.elements[self.next];
        let next = element.next as usize;
        if self.next != next {
            self.next = next;
            Some(JsonValue {
                json: self.json,
                inner: element.value,
            })
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
    type Item = (&'a str, JsonValue<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let member = self.json.members[self.next];
        let next = member.next as usize;
        if self.next != next {
            self.next = next;
            Some((
                self.json.get_string(member.key),
                JsonValue {
                    json: self.json,
                    inner: member.value,
                },
            ))
        } else {
            None
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
    String(JsonRange),
    Array(JsonRange),
    Object(JsonRange),
}

#[derive(Clone, Copy)]
struct JsonRange {
    from: u32,
    to: u32,
}

#[derive(Clone, Copy)]
struct JsonArrayElement {
    value: JsonValueImpl,
    next: u32,
}

#[derive(Clone, Copy)]
struct JsonObjectMember {
    key: JsonRange,
    value: JsonValueImpl,
    next: u32,
}

#[derive(Default)]
pub struct Json {
    strings: String,
    elements: Vec<JsonArrayElement>,
    members: Vec<JsonObjectMember>,
}

impl Json {
    pub fn parse<'a>(&'a mut self, json: &str) -> JsonValue<'a> {
        self.strings.clear();
        self.elements.clear();
        self.members.clear();

        JsonValue {
            json: self,
            inner: JsonValueImpl::Undefined,
        }
    }

    pub fn create_string<'a>(&'a mut self, value: &str) -> JsonValue<'a> {
        let range = self.create_string_range(value);
        JsonValue {
            json: self,
            inner: JsonValueImpl::String(range),
        }
    }

    pub fn create_array<'a>(&'a mut self) -> JsonValue<'a> {
        JsonValue {
            json: self,
            inner: JsonValueImpl::Array(JsonRange { from: 0, to: 0 }),
        }
    }

    pub fn create_object<'a>(&'a mut self) -> JsonValue<'a> {
        JsonValue {
            json: self,
            inner: JsonValueImpl::Object(JsonRange { from: 0, to: 0 }),
        }
    }

    fn create_string_range(&mut self, value: &str) -> JsonRange {
        let from = self.strings.len() as u32;
        self.strings.push_str(value);
        let to = self.strings.len() as u32;
        JsonRange { from, to }
    }

    fn get_string(&self, range: JsonRange) -> &str {
        &self.strings[(range.from as usize)..(range.to as usize)]
    }
}

fn write_value<W>(json: &Json, writer: &mut W, value: JsonValueImpl) -> io::Result<()>
where
    W: io::Write,
{
    match value {
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
        JsonValueImpl::String(r) => write_str(writer, json.get_string(r))?,
        JsonValueImpl::Array(r) => {
            writer.write(b"[")?;
            let mut next = r.from as usize;
            let end = r.to as usize;
            if next != end {
                loop {
                    let element = json.elements[next];
                    write_value(json, writer, element.value)?;
                    next = element.next as _;
                    if next == end {
                        break;
                    }
                    writer.write(b",")?;
                }
            }
            writer.write(b"]")?;
        }
        JsonValueImpl::Object(r) => {
            writer.write(b"{")?;
            let mut next = r.from as usize;
            let end = r.to as usize;
            if next != end {
                loop {
                    let member = json.members[next];
                    write_str(writer, json.get_string(member.key))?;
                    writer.write(b":")?;
                    write_value(json, writer, member.value)?;
                    next = member.next as _;
                    if next == end {
                        break;
                    }
                    writer.write(b",")?;
                }
            }
            writer.write(b"}")?;
        }
    }
    Ok(())
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
