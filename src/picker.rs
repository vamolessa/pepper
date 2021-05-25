use std::{fmt, str::Chars};

use crate::word_database::{WordDatabase, WordIndicesIter};

#[derive(Clone, Copy)]
pub enum EntrySource {
    Custom(usize),
    WordDatabase(usize),
}

struct FilteredEntry {
    pub source: EntrySource,
    pub score: u32,
}

#[derive(Default)]
pub struct Picker {
    fuzzy_matcher: FuzzyMatcher,
    custom_entries_len: usize,
    custom_entries_buffer: Vec<String>,
    filtered_entries: Vec<FilteredEntry>,

    cursor: Option<usize>,
    scroll: usize,
}

impl Picker {
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn len(&self) -> usize {
        self.filtered_entries.len()
    }

    pub fn move_cursor(&mut self, offset: isize) {
        if self.filtered_entries.is_empty() {
            return;
        }

        let end_index = self.filtered_entries.len() - 1;
        let cursor = match self.cursor {
            Some(ref mut cursor) => cursor,
            None => {
                if self.len() > 0 {
                    self.cursor = Some(0);
                }
                return;
            }
        };

        if offset > 0 {
            let mut offset = offset as usize;
            if *cursor == end_index {
                offset -= 1;
                *cursor = 0;
            }

            if offset < end_index - *cursor {
                *cursor += offset;
            } else {
                *cursor = end_index;
            }
        } else if offset < 0 {
            let mut offset = (-offset) as usize;
            if *cursor == 0 {
                offset -= 1;
                *cursor = end_index;
            }

            if offset < *cursor {
                *cursor -= offset;
            } else {
                *cursor = 0;
            }
        }
    }

    pub fn update_scroll(&mut self, max_height: usize) -> usize {
        let height = self.len().min(max_height);
        let cursor = self.cursor.unwrap_or(0);
        if cursor < self.scroll {
            self.scroll = cursor;
        } else if cursor >= self.scroll + height {
            self.scroll = cursor + 1 - height;
        }
        self.scroll = self
            .scroll
            .min(self.filtered_entries.len().saturating_sub(height));

        height
    }

    pub fn clear(&mut self) {
        self.custom_entries_len = 0;
        self.filtered_entries.clear();
        self.cursor = None;
        self.scroll = 0;
    }

    fn new_custom_entry(&mut self) -> &mut String {
        if self.custom_entries_len == self.custom_entries_buffer.len() {
            self.custom_entries_buffer.push(String::new());
        }
        let entry = &mut self.custom_entries_buffer[self.custom_entries_len];
        self.custom_entries_len += 1;
        entry.clear();
        entry
    }

    pub fn add_custom_entry(&mut self, name: &str) {
        let entry = self.new_custom_entry();
        entry.push_str(name);
    }

    pub fn add_custom_entry_fmt(&mut self, args: fmt::Arguments) {
        let entry = self.new_custom_entry();
        let _ = fmt::write(entry, args);
    }

    pub fn add_custom_entry_filtered(&mut self, name: &str, pattern: &str) {
        self.add_custom_entry(name);
        if self.filter_custom_entry(self.custom_entries_len - 1, pattern) {
            self.filtered_entries
                .sort_unstable_by(|a, b| b.score.cmp(&a.score));
        }
    }

    pub fn filter(&mut self, word_indices: WordIndicesIter, pattern: &str) {
        self.filtered_entries.clear();

        for (i, word) in word_indices {
            let score = self.fuzzy_matcher.score(word, pattern);
            if score != 0 {
                self.filtered_entries.push(FilteredEntry {
                    source: EntrySource::WordDatabase(i),
                    score,
                });
            }
        }

        for i in 0..self.custom_entries_len {
            self.filter_custom_entry(i, pattern);
        }

        self.filtered_entries
            .sort_unstable_by(|a, b| b.score.cmp(&a.score));

        let len = self.filtered_entries.len();
        if len > 0 {
            self.cursor = self.cursor.map(|c| c.min(len - 1));
        } else {
            self.cursor = None;
        }
    }

    fn filter_custom_entry(&mut self, index: usize, pattern: &str) -> bool {
        let entry = &self.custom_entries_buffer[index];
        let score = self.fuzzy_matcher.score(entry, pattern);
        if score == 0 {
            return false;
        }

        self.filtered_entries.push(FilteredEntry {
            source: EntrySource::Custom(index),
            score,
        });
        true
    }

    pub fn current_entry<'a>(&'a self, words: &'a WordDatabase) -> Option<(EntrySource, &'a str)> {
        let entry = &self.filtered_entries[self.cursor?];
        let source = entry.source;
        let entry = filtered_to_picker_entry(entry, &self.custom_entries_buffer, words);
        Some((source, entry))
    }

    pub fn entries<'a>(
        &'a self,
        words: &'a WordDatabase,
    ) -> impl 'a + ExactSizeIterator<Item = &'a str> {
        let custom_entries = &self.custom_entries_buffer[..];
        self.filtered_entries
            .iter()
            .map(move |e| filtered_to_picker_entry(e, custom_entries, words))
    }
}

fn filtered_to_picker_entry<'a>(
    entry: &FilteredEntry,
    custom_entries: &'a [String],
    words: &'a WordDatabase,
) -> &'a str {
    match entry.source {
        EntrySource::Custom(i) => &custom_entries[i],
        EntrySource::WordDatabase(i) => words.word_at(i),
    }
}

const RECURSION_LIMIT: u8 = 8;
const BONUS_WORD_BOUNDARY: u32 = 2;
const BONUS_CONSECUTIVE: u32 = 3;

#[derive(Default)]
struct FuzzyMatcher;
impl FuzzyMatcher {
    pub fn score(&mut self, text: &str, pattern: &str) -> u32 {
        fn recursive(
            mut text: Chars,
            mut last_text_char: char,
            mut pattern: Chars,
            mut pattern_char: char,
            depth: u8,
        ) -> u32 {
            let mut text_char = match text.next() {
                Some(c) => c,
                None => return 0,
            };

            let mut matched;
            let mut on_word_boundary_sequence = false;
            let mut best_score = 0;
            let mut score = 0;

            loop {
                matched = text_char.eq_ignore_ascii_case(&pattern_char);
                if matched {
                    let text_char_is_alphanumeric = text_char.is_ascii_alphanumeric();
                    let is_word_boundary = (!last_text_char.is_ascii_alphanumeric()
                        && text_char_is_alphanumeric)
                        || (last_text_char.is_ascii_lowercase() && text_char.is_ascii_uppercase());

                    if depth < RECURSION_LIMIT {
                        let recursive_score = recursive(
                            text.clone(),
                            last_text_char,
                            pattern.clone(),
                            pattern_char,
                            depth + 1,
                        );
                        if recursive_score != 0 {
                            best_score = best_score.max(recursive_score + score);
                        }
                    }

                    if on_word_boundary_sequence {
                        score += BONUS_CONSECUTIVE;
                    }
                    if is_word_boundary {
                        score += BONUS_WORD_BOUNDARY;
                        on_word_boundary_sequence = true;
                    }

                    if on_word_boundary_sequence || !text_char_is_alphanumeric {
                        pattern_char = match pattern.next() {
                            Some(c) => c,
                            None => break,
                        };
                    }
                } else {
                    on_word_boundary_sequence = false;
                }

                last_text_char = text_char;
                text_char = match text.next() {
                    Some(c) => c,
                    None => {
                        matched = matched && pattern.next().is_none();
                        break;
                    }
                };
            }

            if matched {
                score.max(best_score)
            } else {
                best_score
            }
        }

        let mut pattern_chars = pattern.chars();
        let pattern_char = match pattern_chars.next() {
            Some(c) => c,
            None => return 1,
        };

        let score = recursive(text.chars(), '\0', pattern_chars, pattern_char, 0);
        if score != 0 {
            score + (text.len() == pattern.len()) as u32
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matcher_test() {
        let mut fuzzy_matcher = FuzzyMatcher::default();

        assert_eq!(1, fuzzy_matcher.score("", ""));
        assert_eq!(1, fuzzy_matcher.score("abc", ""));
        assert_eq!(0, fuzzy_matcher.score("", "abc"));
        assert_eq!(0, fuzzy_matcher.score("abc", "z"));
        assert_eq!(0, fuzzy_matcher.score("a", "xyz"));

        assert_eq!(
            BONUS_WORD_BOUNDARY + BONUS_CONSECUTIVE * 3 + 1,
            fuzzy_matcher.score("word", "word"),
        );

        assert_eq!(
            BONUS_WORD_BOUNDARY + BONUS_CONSECUTIVE * 2,
            fuzzy_matcher.score("word", "wor"),
        );

        assert_eq!(0, fuzzy_matcher.score("word", "wrd"),);
        assert_eq!(
            BONUS_WORD_BOUNDARY * 2,
            fuzzy_matcher.score("first/second", "f/s")
        );

        assert_eq!(
            (BONUS_WORD_BOUNDARY + BONUS_CONSECUTIVE) * 2,
            fuzzy_matcher.score("camelCase", "caca"),
        );

        assert_eq!(
            BONUS_WORD_BOUNDARY * 3,
            fuzzy_matcher.score("ababAbA", "aaa")
        );
        assert_eq!(
            BONUS_WORD_BOUNDARY * 2,
            fuzzy_matcher.score("abc cde", "ac"),
        );
        assert_eq!(BONUS_WORD_BOUNDARY, fuzzy_matcher.score("abc x", "x"));

        assert_eq!(
            BONUS_WORD_BOUNDARY + BONUS_CONSECUTIVE * 3,
            fuzzy_matcher.score("AxxBxx Abcd", "abcd")
        );
    }
}

