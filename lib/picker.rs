use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::word_database::WordCollection;

#[derive(Default, Clone, Copy)]
pub struct PickerEntry<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub score: i64,
}

struct CustomPickerEntry {
    pub name: String,
    pub description: String,
}

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
    custom_entries_buffer: Vec<CustomPickerEntry>,
    filtered_entries: Vec<FilteredEntry>,

    cursor: usize,
    scroll: usize,

    has_unfiltered_entries: bool,
}

impl Picker {
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn height(&self, max_height: usize) -> usize {
        self.filtered_entries.len().min(max_height)
    }

    pub fn len(&self) -> usize {
        self.filtered_entries.len()
    }

    pub fn move_cursor(&mut self, offset: isize) {
        if self.filtered_entries.is_empty() {
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

    pub fn update_scroll_and_unfiltered_entries<W>(
        &mut self,
        max_height: usize,
        words: &W,
        pattern: &str,
    ) -> usize
    where
        W: WordCollection,
    {
        if self.has_unfiltered_entries {
            self.filter(words, pattern);
        }

        let height = self.height(max_height);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + height as usize {
            self.scroll = self.cursor + 1 - height as usize;
        }
        self.scroll = self
            .scroll
            .min(self.filtered_entries.len().saturating_sub(height));

        height
    }

    pub fn reset(&mut self) {
        self.clear_filtered();
        self.custom_entries_len = 0;
    }

    pub fn add_custom_entry(&mut self, name: &str, description: &str) {
        if self.custom_entries_len < self.custom_entries_buffer.len() {
            let entry = &mut self.custom_entries_buffer[self.custom_entries_len];
            entry.name.clear();
            entry.name.push_str(name);
            entry.description.clear();
            entry.description.push_str(description);
        } else {
            let entry = CustomPickerEntry {
                name: name.into(),
                description: description.into(),
            };
            self.custom_entries_buffer.push(entry);
        }

        self.custom_entries_len += 1;
        self.has_unfiltered_entries = true;
    }

    pub fn clear_filtered(&mut self) {
        self.has_unfiltered_entries = false;
        self.filtered_entries.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    pub fn filter<W>(&mut self, words: &W, pattern: &str)
    where
        W: WordCollection,
    {
        self.has_unfiltered_entries = false;
        self.filtered_entries.clear();

        for (i, word) in words.word_indices() {
            if let Some(mut score) = self.matcher.fuzzy_match(word, pattern) {
                if word.len() == pattern.len() {
                    score += 1;
                }

                self.filtered_entries.push(FilteredEntry {
                    source: FilteredEntrySource::WordDatabase(i),
                    score,
                });
            }
        }

        for (i, entry) in self.custom_entries_buffer[..self.custom_entries_len]
            .iter()
            .enumerate()
        {
            let name_score = self.matcher.fuzzy_match(&entry.name, pattern);
            let description_score = self.matcher.fuzzy_match(&entry.description, pattern);

            let name_eq_bonus = (entry.name.len() == pattern.len()) as i64;
            let description_eq_bonus = (entry.description.len() == pattern.len()) as i64;

            let score = match (name_score, description_score) {
                (None, None) => continue,
                (None, Some(s)) => s + description_eq_bonus,
                (Some(s), None) => s + name_eq_bonus,
                (Some(a), Some(b)) => (a + name_eq_bonus).max(b + description_eq_bonus),
            };

            self.filtered_entries.push(FilteredEntry {
                source: FilteredEntrySource::Custom(i),
                score,
            });
        }

        self.filtered_entries
            .sort_unstable_by(|a, b| b.score.cmp(&a.score));
        self.cursor = self
            .cursor
            .min(self.filtered_entries.len().saturating_sub(1));
    }

    pub fn current_entry<'a, W>(&'a self, words: &'a W) -> Option<PickerEntry<'a>>
    where
        W: WordCollection,
    {
        let entry = self.filtered_entries.get(self.cursor)?;
        match entry.source {
            FilteredEntrySource::Custom(i) => {
                let e = &self.custom_entries_buffer[i];
                Some(PickerEntry {
                    name: &e.name,
                    description: &e.description,
                    score: entry.score,
                })
            }
            FilteredEntrySource::WordDatabase(i) => {
                Some(PickerEntry {
                    name: words.word_at(i),
                    description: "",
                    score: entry.score,
                })
            }
        }
    }

    pub fn entries<'a, W>(&'a self, words: &'a W) -> impl 'a + ExactSizeIterator<Item = PickerEntry<'a>>
    where
        W: WordCollection,
    {
        self.filtered_entries.iter().map(move |e| match e.source {
            FilteredEntrySource::Custom(i) => {
                let entry = &self.custom_entries_buffer[i];
                PickerEntry {
                    name: &entry.name,
                    description: &entry.description,
                    score: e.score,
                }
            }
            FilteredEntrySource::WordDatabase(i) => PickerEntry {
                name: words.word_at(i),
                description: "",
                score: e.score,
            },
        })
    }
}
