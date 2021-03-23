use std::{
    collections::hash_map::{DefaultHasher, Entry, HashMap},
    hash::{BuildHasher, Hash, Hasher},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordKind {
    Identifier,
    Symbol,
    Whitespace,
}

impl WordKind {
    pub fn from_char(c: char) -> Self {
        if c == '_' || c.is_alphanumeric() {
            Self::Identifier
        } else if c.is_whitespace() {
            Self::Whitespace
        } else {
            Self::Symbol
        }
    }
}

pub struct WordRef<'a> {
    pub kind: WordKind,
    pub text: &'a str,
}

#[derive(Clone)]
pub struct WordIter<'a>(&'a str);
impl<'a> WordIter<'a> {
    pub fn new(text: &'a str) -> Self {
        Self(text)
    }

    #[inline]
    pub fn of_kind(self, kind: WordKind) -> impl DoubleEndedIterator<Item = &'a str> {
        self.filter_map(move |w| if kind == w.kind { Some(w.text) } else { None })
    }
}
impl<'a> Iterator for WordIter<'a> {
    type Item = WordRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut chars = self.0.chars();
        let kind = WordKind::from_char(chars.next()?);
        while let Some(c) = chars.next() {
            if kind != WordKind::from_char(c) {
                let rest_len = chars.as_str().len();
                let (word, rest) = self.0.split_at(self.0.len() - rest_len - c.len_utf8());
                self.0 = rest;
                return Some(WordRef { kind, text: word });
            }
        }

        let word = WordRef { kind, text: self.0 };
        self.0 = "";
        Some(word)
    }
}
impl<'a> DoubleEndedIterator for WordIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let mut chars = self.0.chars();
        let kind = WordKind::from_char(chars.next_back()?);
        while let Some(c) = chars.next_back() {
            if kind != WordKind::from_char(c) {
                let rest_len = chars.as_str().len();
                let (rest, word) = self.0.split_at(rest_len + c.len_utf8());
                self.0 = rest;
                return Some(WordRef { kind, text: word });
            }
        }

        let word = WordRef { kind, text: self.0 };
        self.0 = "";
        Some(word)
    }
}

#[derive(Default)]
struct Word {
    text: String,
    count: usize,
}

#[derive(PartialEq, Eq)]
struct WordHash(u64);
impl WordHash {
    pub fn new(word: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        word.hash(&mut hasher);
        Self(hasher.finish())
    }
}
impl Hash for WordHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0);
    }
}

struct WordHasher(u64);
impl BuildHasher for WordHasher {
    type Hasher = Self;

    fn build_hasher(&self) -> Self::Hasher {
        Self(0)
    }
}
impl Hasher for WordHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!();
    }

    fn write_u64(&mut self, hash: u64) {
        self.0 = hash;
    }
}

pub struct WordIndicesIter<'a> {
    words: &'a [Word],
    next_index: usize,
}
impl<'a> WordIndicesIter<'a> {
    pub fn empty() -> Self {
        Self {
            words: &[],
            next_index: 0,
        }
    }
}
impl<'a> Iterator for WordIndicesIter<'a> {
    type Item = (usize, &'a str);
    fn next(&mut self) -> Option<Self::Item> {
        while self.next_index < self.words.len() {
            let index = self.next_index;
            self.next_index += 1;

            if self.words[index].count > 0 {
                return Some((index, &self.words[index].text));
            }
        }

        None
    }
}

pub struct WordDatabase {
    words: Vec<Word>,
    free_indices: Vec<usize>,
    hash_to_index: HashMap<WordHash, usize, WordHasher>,
}

impl WordDatabase {
    pub fn new() -> Self {
        Self {
            words: Vec::with_capacity(512),
            free_indices: Vec::new(),
            hash_to_index: HashMap::with_hasher(WordHasher(0)),
        }
    }

    pub fn add(&mut self, word: &str) {
        let hash = WordHash::new(word);
        match self.hash_to_index.entry(hash) {
            Entry::Occupied(entry) => {
                let index = *entry.get();
                self.words[index].count += 1;
            }
            Entry::Vacant(entry) => match self.free_indices.pop() {
                Some(index) => {
                    entry.insert(index);
                    let w = &mut self.words[index];
                    w.text.clear();
                    w.text.push_str(word);
                    w.count = 1;
                }
                None => {
                    entry.insert(self.words.len());
                    self.words.push(Word {
                        text: word.into(),
                        count: 1,
                    });
                }
            },
        }
    }

    pub fn remove(&mut self, word: &str) {
        let hash = WordHash::new(word);
        let entry = self.hash_to_index.entry(hash);
        if let Entry::Occupied(entry) = entry {
            let index = *entry.get();
            let w = &mut self.words[index];
            w.count -= 1;
            if w.count == 0 {
                self.free_indices.push(index);
                entry.remove();
            }
        }
    }

    pub fn word_at(&self, index: usize) -> &str {
        &self.words[index].text
    }

    pub fn word_indices(&self) -> WordIndicesIter {
        WordIndicesIter {
            words: &self.words,
            next_index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_iter() {
        fn assert_word(next: Option<WordRef>, kind: WordKind, text: &str) {
            assert_eq!(Some(kind), next.as_ref().map(|w| w.kind));
            assert_eq!(Some(text), next.as_ref().map(|w| w.text));
        }

        let mut iter = WordIter::new("word");
        assert_word(iter.next(), WordKind::Identifier, "word");
        assert!(iter.next().is_none());

        let mut iter = WordIter::new("first  $#second \tthird!?+");
        assert_word(iter.next(), WordKind::Identifier, "first");
        assert_word(iter.next(), WordKind::Whitespace, "  ");
        assert_word(iter.next(), WordKind::Symbol, "$#");
        assert_word(iter.next(), WordKind::Identifier, "second");
        assert_word(iter.next(), WordKind::Whitespace, " \t");
        assert_word(iter.next(), WordKind::Identifier, "third");
        assert_word(iter.next(), WordKind::Symbol, "!?+");
        assert!(iter.next().is_none());

        let mut iter = WordIter::new("first  $#second \tthird!?+");
        assert_word(iter.next_back(), WordKind::Symbol, "!?+");
        assert_word(iter.next_back(), WordKind::Identifier, "third");
        assert_word(iter.next_back(), WordKind::Whitespace, " \t");
        assert_word(iter.next_back(), WordKind::Identifier, "second");
        assert_word(iter.next_back(), WordKind::Symbol, "$#");
        assert_word(iter.next_back(), WordKind::Whitespace, "  ");
        assert_word(iter.next_back(), WordKind::Identifier, "first");
        assert!(iter.next_back().is_none());
    }

    #[test]
    fn identifier_word_iter() {
        let mut iter = WordIter::new("word").of_kind(WordKind::Identifier);
        assert_eq!(Some("word"), iter.next());
        assert_eq!(None, iter.next());

        let mut iter = WordIter::new("first second third").of_kind(WordKind::Identifier);
        assert_eq!(Some("first"), iter.next());
        assert_eq!(Some("second"), iter.next());
        assert_eq!(Some("third"), iter.next());
        assert_eq!(None, iter.next());

        let mut iter =
            WordIter::new("  1first:second00+?$%third  ^@").of_kind(WordKind::Identifier);
        assert_eq!(Some("1first"), iter.next());
        assert_eq!(Some("second00"), iter.next());
        assert_eq!(Some("third"), iter.next());
        assert_eq!(None, iter.next());
    }

    #[test]
    fn word_database_insert_remove() {
        fn unique_word_count(word_database: &WordDatabase) -> usize {
            word_database.words.len() - word_database.free_indices.len()
        }

        let mut words = WordDatabase::new();

        words.add("first");
        assert_eq!(1, unique_word_count(&words));

        words.add("first");
        words.add("first");
        assert_eq!(1, unique_word_count(&words));

        words.add("second");
        assert_eq!(2, unique_word_count(&words));

        words.remove("first");
        assert_eq!(2, unique_word_count(&words));

        words.remove("first");
        words.remove("first");
        assert_eq!(1, unique_word_count(&words));

        words.remove("first");
        assert_eq!(1, unique_word_count(&words));
    }
}
