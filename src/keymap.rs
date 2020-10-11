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

        let maps = self.maps.entry(mode).or_insert_with(Vec::new);

        for m in maps.iter_mut() {
            if m.from == map.from {
                m.to = map.to;
                return Ok(());
            }
        }

        maps.push(map);
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

        let normal_mode = Mode::Normal(Default::default()).discriminant();
        let insert_mode = Mode::Insert(Default::default()).discriminant();

        let all_modes = [
            normal_mode,
            insert_mode,
            Mode::Search(Default::default()).discriminant(),
            Mode::Picker(Default::default()).discriminant(),
            Mode::Goto(Default::default()).discriminant(),
            Mode::Script(Default::default()).discriminant(),
        ];

        for mode in &all_modes {
            let mode = *mode;
            this.parse_and_map(mode, "<c-c>", "<esc>").unwrap();
            this.parse_and_map(mode, "<c-m>", "<enter>").unwrap();
        }

        this.parse_and_map(normal_mode, "<esc>", "<esc>xcxv/<esc>")
            .unwrap();
        this.parse_and_map(normal_mode, "<c-c>", "<esc>xcxv/<esc>")
            .unwrap();

        this.parse_and_map(normal_mode, "s", "/").unwrap();
        this.parse_and_map(normal_mode, "S", "?").unwrap();

        this.parse_and_map(normal_mode, "gi", "ghw").unwrap();
        this.parse_and_map(normal_mode, "#", "gg").unwrap();
        this.parse_and_map(normal_mode, "I", "xvgii").unwrap();
        this.parse_and_map(normal_mode, "<c-i>", "xvgli").unwrap();
        this.parse_and_map(normal_mode, "J", "xvjgiVkgli<space><esc>")
            .unwrap();

        this.parse_and_map(normal_mode, "o", "xvgli<enter>")
            .unwrap();
        this.parse_and_map(normal_mode, "O", "xvghi<enter><up>")
            .unwrap();

        this.parse_and_map(normal_mode, "aa", "xcxvgkVgjv").unwrap();
        this.parse_and_map(normal_mode, "xs", "x/").unwrap();

        this.parse_and_map(insert_mode, "<c-h>", "<backspace>")
            .unwrap();

        this
    }
}
