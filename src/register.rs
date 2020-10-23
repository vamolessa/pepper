use std::fmt;

pub const SEARCH_REGISTER: RegisterKey = RegisterKey(b's');
pub const KEY_QUEUE_REGISTER: RegisterKey = RegisterKey(b'k');

pub struct RegisterKey(u8);

impl RegisterKey {
    pub const fn from_u8(key: u8) -> Option<RegisterKey> {
        if key >= b'a' && key <= b'z' {
            Some(Self(key - b'a'))
        } else {
            None
        }
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

    pub fn push_fmt(&mut self, key: RegisterKey, args: fmt::Arguments) {
        let register = &mut self.registers[key.0 as usize];
        let _ = fmt::write(register, args);
    }

    pub fn set(&mut self, key: RegisterKey, value: &str) {
        let register = &mut self.registers[key.0 as usize];
        register.clear();
        register.push_str(value);
    }
}
