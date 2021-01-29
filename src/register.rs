use std::fmt;

pub const SEARCH_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('s');
pub const KEY_QUEUE_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('k');
pub const AUTO_MACRO_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('a');

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegisterKey(u8);

impl RegisterKey {
    const fn from_char_unchecked(key: char) -> RegisterKey {
        let key = key as u8;
        Self(key - b'a')
    }

    pub const fn from_char(key: char) -> Option<RegisterKey> {
        let key = key as u8;
        if key >= b'a' && key <= b'z' {
            Some(Self(key - b'a'))
        } else {
            None
        }
    }

    pub fn as_u8(&self) -> u8 {
        self.0 + b'a'
    }
}

#[derive(Default)]
pub struct RegisterCollection {
    registers: [String; (b'z' - b'a' + 1) as usize],
}

impl RegisterCollection {
    pub fn get(&self, key: RegisterKey) -> &str {
        &self.registers[key.0 as usize]
    }

    pub fn append_fmt(&mut self, key: RegisterKey, args: fmt::Arguments) {
        let register = &mut self.registers[key.0 as usize];
        let _ = fmt::write(register, args);
    }

    pub fn set(&mut self, key: RegisterKey, value: &str) {
        let register = &mut self.registers[key.0 as usize];
        register.clear();
        register.push_str(value);
    }
}
