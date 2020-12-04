use std::fmt;

pub const SEARCH_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('s');
pub const KEY_QUEUE_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('k');
pub const AUTO_MACRO_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('a');

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegisterKey(usize);

impl RegisterKey {
    const fn from_char_unchecked(key: char) -> RegisterKey {
        let key = key as usize;
        Self(key - b'a' as usize)
    }

    pub const fn from_char(key: char) -> Option<RegisterKey> {
        let key = key as usize;
        if key >= b'a' as usize && key <= b'z' as usize {
            Some(Self(key - b'a' as usize))
        } else {
            None
        }
    }

    pub const fn to_char(&self) -> char {
        (self.0 as u8 + b'a') as _
    }
}

#[derive(Default)]
pub struct RegisterCollection {
    registers: [String; (b'z' - b'a' + 1) as usize],
}

impl RegisterCollection {
    pub fn get(&self, key: RegisterKey) -> &str {
        &self.registers[key.0]
    }

    pub fn append_fmt(&mut self, key: RegisterKey, args: fmt::Arguments) {
        let register = &mut self.registers[key.0];
        let _ = fmt::write(register, args);
    }

    pub fn set(&mut self, key: RegisterKey, value: &str) {
        let register = &mut self.registers[key.0];
        register.clear();
        register.push_str(value);
    }
}
