use std::collections::HashMap;

use crate::{
    events::{KeyParseAllError, KeyParser},
    mode::ModeKind,
    platform::Key,
};

pub enum MatchResult<'a> {
    None,
    Prefix,
    ReplaceWith(&'a [Key]),
}

#[derive(Debug)]
pub enum ParseKeyMapError {
    From(KeyParseAllError),
    To(KeyParseAllError),
}

struct KeyMap {
    from: Vec<Key>,
    to: Vec<Key>,
}

#[derive(Default)]
pub struct KeyMapCollection {
    maps: HashMap<ModeKind, Vec<KeyMap>>,
}

impl KeyMapCollection {
    pub fn parse_and_map(
        &mut self,
        mode_kind: ModeKind,
        from: &str,
        to: &str,
    ) -> Result<(), ParseKeyMapError> {
        fn parse_keys(text: &str) -> Result<Vec<Key>, KeyParseAllError> {
            let mut keys = Vec::new();
            for key in KeyParser::new(text) {
                match key {
                    Ok(key) => keys.push(key),
                    Err(error) => return Err(error),
                }
            }
            Ok(keys)
        }

        let map = KeyMap {
            from: parse_keys(from).map_err(|e| ParseKeyMapError::From(e))?,
            to: parse_keys(to).map_err(|e| ParseKeyMapError::To(e))?,
        };

        let maps = self.maps.entry(mode_kind).or_insert_with(Vec::new);

        for m in maps.iter_mut() {
            if m.from == map.from {
                m.to = map.to;
                return Ok(());
            }
        }

        maps.push(map);
        Ok(())
    }

    pub fn matches<'a>(&'a self, mode_kind: ModeKind, keys: &[Key]) -> MatchResult<'a> {
        let maps = match self.maps.get(&mode_kind) {
            Some(maps) => maps,
            None => return MatchResult::None,
        };

        let mut has_prefix = false;
        for map in maps {
            if map.from.iter().zip(keys.iter()).all(|(a, b)| a == b) {
                has_prefix = true;
                if map.from.len() == keys.len() {
                    return MatchResult::ReplaceWith(&map.to);
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

