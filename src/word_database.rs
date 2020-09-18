use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharKind {
    Word,
    Symbol,
    Whitespace,
}

impl CharKind {
    pub fn new(c: char) -> Self {
        if c == '_' || c.is_alphanumeric() {
            Self::Word
        } else if c.is_whitespace() {
            Self::Whitespace
        } else {
            Self::Symbol
        }
    }
}

pub struct WordIter<'a>(&'a str);

impl<'a> WordIter<'a> {
    pub fn new(text: &'a str) -> Self {
        Self(text)
    }
}

impl<'a> Iterator for WordIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.0.find(|c| CharKind::new(c) == CharKind::Word)?;
        self.0 = &self.0[start..];

        let end = self
            .0
            .find(|c| CharKind::new(c) != CharKind::Word)
            .unwrap_or(self.0.len());
        let (word, rest) = self.0.split_at(end);

        self.0 = rest;
        Some(word)
    }
}

#[derive(Default)]
struct Word {
    text: String,
    count: usize,
}

pub struct WordDatabase {
    len: usize,
    words: Vec<Word>,
}

impl WordDatabase {
    pub fn empty() -> &'static Self {
        static EMPTY_DATABASE: WordDatabase = WordDatabase {
            len: 0,
            words: Vec::new(),
        };

        &EMPTY_DATABASE
    }

    pub fn new() -> Self {
        let mut words = Vec::with_capacity(4 * 1024);
        words.resize_with(words.capacity(), || Word::default());

        Self { len: 0, words }
    }

    pub fn add_word(&mut self, word: &str) {
        const LOAD_FACTOR_PERCENT: usize = 70;

        if self.len * 100 >= self.words.len() * LOAD_FACTOR_PERCENT {
            let mut words = Vec::with_capacity(self.words.capacity() * 2);
            words.resize_with(words.capacity(), || Word::default());

            std::mem::swap(&mut words, &mut self.words);

            self.len = 0;
            for word in &words {
                if word.count > 0 {
                    self.add_word(&word.text);
                }
            }
        }

        {
            let word_in_bucket = Self::get_word_in_bucket(&mut self.words, word);

            if word_in_bucket.count == 0 {
                word_in_bucket.text.clear();
                word_in_bucket.text.push_str(word);
                self.len += 1;
            }

            word_in_bucket.count += 1;
        }
    }

    pub fn remove_word(&mut self, word: &str) {
        let word_in_bucket = Self::get_word_in_bucket(&mut self.words, word);
        if word_in_bucket.count > 0 {
            word_in_bucket.count -= 1;

            if word_in_bucket.count == 0 {
                self.len -= 1;
            }
        }
    }

    pub fn word_at(&self, index: usize) -> &str {
        &self.words[index].text
    }

    pub fn word_indices<'a>(&'a self) -> impl Iterator<Item = (usize, &'a str)> {
        self.words
            .iter()
            .enumerate()
            .filter(|(_, w)| w.count > 0)
            .map(|(i, w)| (i, &w.text[..]))
    }

    fn get_word_in_bucket<'a>(words: &'a mut [Word], word: &str) -> &'a mut Word {
        let mut hasher = DefaultHasher::new();
        word.hash(&mut hasher);
        let hash = hasher.finish() as usize;

        let bucket_count = words.len();
        &mut words[hash % bucket_count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_iter() {
        let mut iter = WordIter::new("word");
        assert_eq!(Some("word"), iter.next());
        assert_eq!(None, iter.next());

        let mut iter = WordIter::new("first second third");
        assert_eq!(Some("first"), iter.next());
        assert_eq!(Some("second"), iter.next());
        assert_eq!(Some("third"), iter.next());
        assert_eq!(None, iter.next());

        let mut iter = WordIter::new("  1first:second00+?$%third  ^@");
        assert_eq!(Some("1first"), iter.next());
        assert_eq!(Some("second00"), iter.next());
        assert_eq!(Some("third"), iter.next());
        assert_eq!(None, iter.next());
    }

    #[test]
    fn word_database_insert_remove() {
        let mut words = WordDatabase::new();

        words.add_word("first");
        assert_eq!(1, words.len);

        words.add_word("first");
        words.add_word("first");
        assert_eq!(1, words.len);

        words.add_word("second");
        assert_eq!(2, words.len);

        words.remove_word("first");
        assert_eq!(2, words.len);

        words.remove_word("first");
        words.remove_word("first");
        assert_eq!(1, words.len);

        words.remove_word("first");
        assert_eq!(1, words.len);
    }
}
