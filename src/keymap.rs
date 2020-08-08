use std::{collections::HashMap, mem::Discriminant};

use crate::{
    event::{Key, KeyParseError},
    mode::Mode,
};

pub enum MatchResult<'a> {
    None,
    Prefix,
    ReplaceWith(&'a [Key]),
}

pub enum ParseKeyMapError {
    From(usize, KeyParseError),
    To(usize, KeyParseError),
}

struct KeyMap {
    from: Vec<Key>,
    to: Vec<Key>,
}

#[derive(Default)]
pub struct KeyMapCollection {
    maps: HashMap<Discriminant<Mode>, Vec<KeyMap>>,
}

impl KeyMapCollection {
    pub fn parse_map(
        &mut self,
        mode: Discriminant<Mode>,
        from: &str,
        to: &str,
    ) -> Result<(), ParseKeyMapError> {
        fn parse_keys(text: &str) -> Result<Vec<Key>, (usize, KeyParseError)> {
            let mut keys = Vec::new();

            let mut chars = text.chars().peekable();
            while chars.peek().is_some() {
                match Key::parse(&mut chars) {
                    Ok(key) => keys.push(key),
                    Err(error) => {
                        let chars_len: usize = chars.map(|c| c.len_utf8()).sum();
                        let error_index = text.len() - chars_len;
                        return Err((error_index, error));
                    }
                }
            }

            Ok(keys)
        }

        let map = KeyMap {
            from: parse_keys(from).map_err(|(i, e)| ParseKeyMapError::From(i, e))?,
            to: parse_keys(to).map_err(|(i, e)| ParseKeyMapError::To(i, e))?,
        };

        self.maps.entry(mode).or_insert_with(Vec::new).push(map);
        Ok(())
    }

    pub fn matches<'a>(&'a self, mode: Discriminant<Mode>, keys: &[Key]) -> MatchResult<'a> {
        let maps = match self.maps.get(&mode) {
            Some(maps) => maps,
            None => return MatchResult::None,
        };

        let mut has_prefix = false;
        for map in maps {
            if map.from.iter().zip(keys.iter()).all(|(a, b)| a == b) {
                has_prefix = true;
                if map.from.len() == keys.len() {
                    return MatchResult::ReplaceWith(&map.to[..]);
                }
            }
        }

        if has_prefix {
            MatchResult::Prefix
        } else {
            MatchResult::None
        }
    }
}
