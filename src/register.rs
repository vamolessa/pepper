use std::fmt;

pub const SEARCH_REGISTER: RegisterKey = RegisterKey((b's' - b'a') as usize);
pub const KEY_QUEUE_REGISTER: RegisterKey = RegisterKey((b'k' - b'a') as usize);

#[derive(Clone, Copy)]
pub struct RegisterKey(usize);

impl RegisterKey {
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
