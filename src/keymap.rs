use std::{collections::HashMap, mem::Discriminant};

use crate::{
    event::{Key, KeyParseError},
    mode::Mode,
};

pub enum MatchResult<'a> {
    None,
    Prefix,
    Replace(&'a [Key]),
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
    ) -> Result<(), KeyParseError> {
        fn parse_keys(text: &str) -> Result<Vec<Key>, KeyParseError> {
            let mut keys = Vec::new();

            let mut chars = text.chars().peekable();
            while chars.peek().is_some() {
                keys.push(Key::parse(&mut chars)?);
            }

            Ok(keys)
        }

        let map = KeyMap {
            from: parse_keys(from)?,
            to: parse_keys(to)?,
        };

        self.maps.entry(mode).or_insert(Vec::new()).push(map);
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
                    return MatchResult::Replace(&map.to[..]);
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
