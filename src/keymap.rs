use std::collections::HashMap;

use crate::{event::Key, mode::Mode};

struct KeyMap {
    from: Vec<Key>,
    to: Vec<Key>,
}

#[derive(Default)]
pub struct KeyMapCollection {
    maps: HashMap<Mode, Vec<KeyMap>>,
}
