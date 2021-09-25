use std::fmt;

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
impl ParseKeyMapError {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::From(_) => "invalid 'from' binding",
            Self::To(_) => "invalid 'to' binding",
        }
    }
}
impl fmt::Display for ParseKeyMapError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::From(error) => write!(f, "invalid 'from' binding '{}'", error),
            Self::To(error) => write!(f, "invalid 'to' binding '{}'", error),
        }
    }
}

struct KeyMap {
    from: Vec<Key>,
    to: Vec<Key>,
}

#[derive(Default)]
pub struct KeyMapCollection {
    maps: [Vec<KeyMap>; 5],
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
            from: parse_keys(from).map_err(ParseKeyMapError::From)?,
            to: parse_keys(to).map_err(ParseKeyMapError::To)?,
        };

        let maps = &mut self.maps[mode_kind as usize];
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
        let maps = &self.maps[mode_kind as usize];

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
