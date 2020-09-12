use std::collections::hash_map::RandomState;

#[derive(Default)]
struct Word {
    text: String,
    count: usize,
}

#[derive(Default)]
pub struct WordDatabase {
    hasher_builder: RandomState,
    words: Vec<Word>,
}

impl WordDatabase {
}
