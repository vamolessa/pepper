pub const SEARCH_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('s');
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

    pub fn from_str(key: &str) -> Option<RegisterKey> {
        let key = key.as_bytes();
        if key.len() == 1 {
            Self::from_char(key[0] as _)
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

    pub fn get_mut(&mut self, key: RegisterKey) -> &mut String {
        &mut self.registers[key.0 as usize]
    }
}
