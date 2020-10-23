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
        JsonValue {
            json: self,
            inner: JsonValueImpl::Undefined,
        }
    }

    fn get_string<'a>(&'a self, range: JsonStringRange) -> &'a str {
        &self.strings[range.from..range.to]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
