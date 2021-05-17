use std::{fmt, str::Chars};

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::word_database::{WordDatabase, WordIndicesIter};

#[derive(Clone, Copy)]
pub enum EntrySource {
    Custom(usize),
    WordDatabase(usize),
}

struct FilteredEntry {
    pub source: EntrySource,
    pub score: i64,
}

#[derive(Default)]
pub struct Picker {
    matcher: SkimMatcherV2,

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

    pub fn fuzzy_match(&self, text: &str, pattern: &str) -> Option<i64> {
        let score = self.matcher.fuzzy_match(text, pattern)?;
        let score = score + (text.len() == pattern.len()) as i64;
        Some(score)
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
            if let Some(score) = self.fuzzy_match(word, pattern) {
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
        let score = match self.fuzzy_match(entry, pattern) {
            Some(score) => score,
            None => return false,
        };

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

const MIN_SCORE: i64 = i64::MIN;
const RECURSION_LIMIT: u8 = 8;
const BONUS_WORD_BOUNDARY: i64 = 30;
const BONUS_CONSECUTIVE: i64 = 15;
const PENALTY_LEADING_UNMATCHED: i64 = -5;
const PENALTY_LEADING_UNMATCHED_MAX: u8 = 3;
const PENALTY_UNMATCHED: i64 = -1;

fn fuzzy_match(text: &str, pattern: &str) -> i64 {
    fn recursive(mut text: Chars, mut pattern: Chars, mut last_text_char: char, depth: u8) -> i64 {
        let mut text_char = match text.next() {
            Some(c) => c,
            None => return MIN_SCORE,
        };
        let mut pattern_char = match pattern.next() {
            Some(c) => c,
            None => return MIN_SCORE,
        };

        let mut had_match = false;
        let mut last_was_matched = false;
        let mut leading_unmatched_len = if depth == 0 {
            PENALTY_LEADING_UNMATCHED_MAX
        } else {
            0
        };

        let mut best_score = MIN_SCORE;
        let mut score = 0;

        loop {
            let matched = text_char.eq_ignore_ascii_case(&pattern_char);
            if matched {
                had_match = true;
                leading_unmatched_len = 0;
                if !last_text_char.is_ascii_alphabetic() && text_char.is_ascii_alphabetic() {
                    score += BONUS_WORD_BOUNDARY;
                }
                if last_text_char.is_ascii_lowercase() && text_char.is_ascii_uppercase() {
                    score += BONUS_WORD_BOUNDARY;
                }
                if last_was_matched {
                    score += BONUS_CONSECUTIVE;
                }

                if depth < RECURSION_LIMIT {
                    let score = recursive(text.clone(), pattern.clone(), last_text_char, depth + 1);
                    best_score = best_score.max(score.saturating_add(PENALTY_UNMATCHED));
                }

                pattern_char = match pattern.next() {
                    Some(c) => c,
                    None => break,
                };
            } else {
                if leading_unmatched_len > 0 {
                    score += PENALTY_LEADING_UNMATCHED;
                    leading_unmatched_len -= 1;
                }
                score += PENALTY_UNMATCHED;
            }

            last_was_matched = matched;
            last_text_char = text_char;
            text_char = match text.next() {
                Some(c) => c,
                None => break,
            };
        }

        match pattern.next() {
            None if had_match => {
                score += text.count() as i64 * PENALTY_UNMATCHED;
                score.max(best_score)
            }
            _ => best_score,
        }
    }

    let score = recursive(text.chars(), pattern.chars(), '\0', 0);
    if score != MIN_SCORE {
        score + (text.len() == pattern.len()) as i64
    } else {
        MIN_SCORE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_test() {
        assert_eq!(MIN_SCORE, fuzzy_match("", ""));
        assert_eq!(MIN_SCORE, fuzzy_match("abc", ""));
        assert_eq!(MIN_SCORE, fuzzy_match("", "abc"));
        assert_eq!(MIN_SCORE, fuzzy_match("abc", "z"));
        assert_eq!(MIN_SCORE, fuzzy_match("a", "xyz"));

        assert_eq!(
            BONUS_WORD_BOUNDARY + BONUS_CONSECUTIVE * 3 + 1,
            fuzzy_match("word", "word"),
        );

        assert_eq!(
            BONUS_WORD_BOUNDARY + BONUS_CONSECUTIVE * 2 + PENALTY_UNMATCHED,
            fuzzy_match("word", "wor"),
        );

        assert_eq!(
            BONUS_WORD_BOUNDARY + PENALTY_UNMATCHED + BONUS_CONSECUTIVE,
            fuzzy_match("word", "wrd"),
        );

        assert_eq!(
            BONUS_WORD_BOUNDARY
                + BONUS_CONSECUTIVE
                + PENALTY_UNMATCHED * 3
                + BONUS_WORD_BOUNDARY
                + BONUS_CONSECUTIVE
                + PENALTY_UNMATCHED * 2,
            fuzzy_match("camelCase", "caca"),
        );

        assert_eq!(
            BONUS_WORD_BOUNDARY
                + PENALTY_UNMATCHED * 3
                + BONUS_WORD_BOUNDARY
                + PENALTY_UNMATCHED
                + BONUS_WORD_BOUNDARY,
            fuzzy_match("ababAbA", "aaa"),
        );
    }
}

