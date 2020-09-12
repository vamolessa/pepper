use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::word_database::WordDatabase;

#[derive(Default, Clone, Copy)]
pub struct PickerEntry<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub score: i64,
}

pub struct CustomPickerEntry {
    pub name: String,
    pub description: String,
}

enum FiletedEntrySource {
    Custom(usize),
    WordDatabase(usize),
}

struct FilteredEntry {
    pub source: FiletedEntrySource,
    pub score: i64,
}

pub struct Picker {
    matcher: SkimMatcherV2,
    custom_entries: Vec<CustomPickerEntry>,
    filtered_entries: Vec<FilteredEntry>,

    cursor: usize,
    scroll: usize,

    cached_current_word: String,
}

impl Picker {
    pub fn new() -> Self {
        let mut matcher = SkimMatcherV2::default();

        Self {
            matcher,
            custom_entries: Vec::new(),
            filtered_entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            cached_current_word: String::new(),
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn height(&self, max_height: usize) -> usize {
        self.filtered_entries.len().min(max_height)
    }

    pub fn move_cursor(&mut self, offset: isize) {
        if self.filtered_entries.len() == 0 {
            return;
        }

        let end_index = self.filtered_entries.len() - 1;

        if offset > 0 {
            let mut offset = offset as usize;
            if self.cursor == end_index {
                offset -= 1;
                self.cursor = 0;
            }

            if offset < end_index - self.cursor {
                self.cursor += offset;
            } else {
                self.cursor = end_index;
            }
        } else if offset < 0 {
            let mut offset = (-offset) as usize;
            if self.cursor == 0 {
                offset -= 1;
                self.cursor = end_index;
            }

            if offset < self.cursor {
                self.cursor -= offset;
            } else {
                self.cursor = 0;
            }
        }
    }

    pub fn update_scroll(&mut self, max_height: usize) {
        let height = self.height(max_height);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + height as usize {
            self.scroll = self.cursor + 1 - height as usize;
        }
    }

    pub fn reset(&mut self) {
        self.clear_filtered();
        self.custom_entries.clear();
    }

    pub fn add_custom_entry(&mut self, entry: CustomPickerEntry) {
        self.custom_entries.push(entry);
    }

    pub fn clear_filtered(&mut self) {
        self.filtered_entries.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    pub fn filter(&mut self, word_database: &WordDatabase, pattern: &str) {
        self.filtered_entries.clear();

        for (i, word) in word_database.word_indices() {
            if let Some(mut score) = self.matcher.fuzzy_match(word, pattern) {
                if word.len() == pattern.len() {
                    score += 1;
                }

                self.filtered_entries.push(FilteredEntry {
                    source: FiletedEntrySource::WordDatabase(i),
                    score,
                });
            }
        }

        for (i, entry) in self.custom_entries.iter().enumerate() {
            if let Some(mut score) = self.matcher.fuzzy_match(&entry.name, pattern) {
                if entry.name.len() == pattern.len() {
                    score += 1;
                }

                self.filtered_entries.push(FilteredEntry {
                    source: FiletedEntrySource::Custom(i),
                    score,
                });
            }
        }

        self.filtered_entries
            .sort_unstable_by(|a, b| b.score.cmp(&a.score));
        self.cursor = self.cursor.min(self.filtered_entries.len());
    }

    pub fn current_entry_name<'a>(&'a mut self, word_database: &WordDatabase) -> &'a str {
        let entry = &self.filtered_entries[self.cursor];
        match entry.source {
            FiletedEntrySource::Custom(i) => &self.custom_entries[i].name,
            FiletedEntrySource::WordDatabase(i) => {
                let word = word_database.word_at(i);
                self.cached_current_word.clear();
                self.cached_current_word.push_str(word);
                &self.cached_current_word
            }
        }
    }

    pub fn entries<'a>(
        &'a self,
        word_database: &'a WordDatabase,
    ) -> impl 'a + Iterator<Item = PickerEntry<'a>> {
        self.filtered_entries.iter().map(move |e| match e.source {
            FiletedEntrySource::Custom(i) => {
                let entry = &self.custom_entries[i];
                PickerEntry {
                    name: &entry.name,
                    description: &entry.description,
                    score: e.score,
                }
            }
            FiletedEntrySource::WordDatabase(i) => {
                let word = word_database.word_at(i);
                PickerEntry {
                    name: word,
                    description: "",
                    score: e.score,
                }
            }
        })
    }
}
