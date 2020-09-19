use std::{collections::HashMap, mem::Discriminant};

use crate::{
    client_event::{Key, KeyParseAllError},
    mode::Mode,
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

pub struct KeyMapCollection {
    maps: HashMap<Discriminant<Mode>, Vec<KeyMap>>,
}

impl KeyMapCollection {
    pub fn parse_and_map(
        &mut self,
        mode: Discriminant<Mode>,
        from: &str,
        to: &str,
    ) -> Result<(), ParseKeyMapError> {
        fn parse_keys(text: &str) -> Result<Vec<Key>, KeyParseAllError> {
            let mut keys = Vec::new();
            for key in Key::parse_all(text) {
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

impl Default for KeyMapCollection {
    fn default() -> Self {
        let mut this = Self {
            maps: HashMap::default(),
        };

        let all_modes = [
            Mode::Normal(Default::default()),
            Mode::Insert,
            Mode::Search,
            Mode::Script,
        ];

        for mode in &all_modes {
            this.parse_and_map(mode.discriminant(), "<c-c>", "<esc>")
                .unwrap();
            this.parse_and_map(mode.discriminant(), "<c-m>", "<enter>")
                .unwrap();
        }

        this.parse_and_map(Mode::Insert.discriminant(), "<c-h>", "<backspace>")
            .unwrap();

        this
    }
}
