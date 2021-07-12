pub static SEARCH_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('s');
pub static AUTO_MACRO_REGISTER: RegisterKey = RegisterKey::from_char_unchecked('a');

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegisterKey(u8);

impl RegisterKey {
    const fn from_char_unchecked(key: char) -> Self {
        let key = key as u8;
        Self(key - b'a')
    }

    pub const fn from_char(key: char) -> Option<Self> {
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

const REGISTERS_LEN: usize = (b'z' - b'a' + 1) as _;

pub struct RegisterCollection {
    registers: [String; REGISTERS_LEN],
}

impl RegisterCollection {
    pub const fn new() -> Self {
        const DEFAULT_STRING: String = String::new();
        Self {
            registers: [DEFAULT_STRING; REGISTERS_LEN],
        }
    }

    pub fn get(&self, key: RegisterKey) -> &str {
        &self.registers[key.0 as usize]
    }

    pub fn get_mut(&mut self, key: RegisterKey) -> &mut String {
        &mut self.registers[key.0 as usize]
    }
}
