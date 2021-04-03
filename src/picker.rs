use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::word_database::{WordDatabase, WordIndicesIter};

enum FilteredEntrySource {
    Custom(usize),
    WordDatabase(usize),
}

struct FilteredEntry {
    pub source: FilteredEntrySource,
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
        self.clear_filtered();
        self.custom_entries_len = 0;
    }

    pub fn add_custom_entry(&mut self, name: &str) {
        if self.custom_entries_len < self.custom_entries_buffer.len() {
            let entry = &mut self.custom_entries_buffer[self.custom_entries_len];
            entry.clear();
            entry.push_str(name);
        } else {
            self.custom_entries_buffer.push(name.into());
        }

        self.custom_entries_len += 1;
    }

    pub fn add_custom_entry_filtered(&mut self, name: &str, pattern: &str) {
        self.add_custom_entry(name);
        if self.filter_custom_entry(self.custom_entries_len - 1, pattern) {
            self.filtered_entries
                .sort_unstable_by(|a, b| b.score.cmp(&a.score));
        }
    }

    pub fn clear_filtered(&mut self) {
        self.filtered_entries.clear();
        self.cursor = None;
        self.scroll = 0;
    }

    pub fn filter(&mut self, word_indices: WordIndicesIter, pattern: &str) {
        self.filtered_entries.clear();

        for (i, word) in word_indices {
            if let Some(score) = self.fuzzy_match(word, pattern) {
                self.filtered_entries.push(FilteredEntry {
                    source: FilteredEntrySource::WordDatabase(i),
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
            source: FilteredEntrySource::Custom(index),
            score,
        });
        true
    }

    pub fn current_entry<'a>(&'a self, words: &'a WordDatabase) -> Option<&'a str> {
        let entry = &self.filtered_entries[self.cursor?];
        let entry = filtered_to_picker_entry(entry, &self.custom_entries_buffer, words);
        Some(entry)
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
        FilteredEntrySource::Custom(i) => &custom_entries[i],
        FilteredEntrySource::WordDatabase(i) => words.word_at(i),
    }
}
