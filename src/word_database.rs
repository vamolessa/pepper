use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

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
            .filter(|(_i, w)| w.count > 0)
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
