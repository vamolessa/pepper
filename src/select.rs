use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

pub trait SelectEntryProvider {
    fn provide_entries(&self) -> &[SelectEntry];
}

impl<'a> SelectEntryProvider for &[SelectEntry<'a>] {
    fn provide_entries(&self) -> &[SelectEntry] {
        self
    }
}

#[derive(Default)]
pub struct SelectEntry<'a> {
    pub name: &'a str,
    pub description: &'a str,
}

impl<'a> SelectEntry<'a> {
    pub const fn from_str(name: &'a str) -> Self {
        Self {
            name,
            description: "",
        }
    }
}

#[derive(Default)]
struct FilteredEntry {
    pub provider_index: usize,
    pub entry_index: usize,
    pub score: i64,
}

#[derive(Default)]
pub struct SelectEntryCollection {
    cursor: usize,
    scroll: usize,

    providers: Vec<Box<dyn SelectEntryProvider>>,
    filtered: Vec<FilteredEntry>,
    filter: String,
    matcher: SkimMatcherV2,
}

impl SelectEntryCollection {
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn height(&self, max_height: usize) -> usize {
        self.filtered.len().min(max_height)
    }

    pub fn get_filter(&self) -> &str {
        &self.filter[..]
    }

    pub fn move_cursor(&mut self, offset: isize) {
        if self.filtered.len() == 0 {
            return;
        }

        let last_index = self.filtered.len() - 1;
        if offset > 0 {
            let mut offset = offset as usize;
            if self.cursor == last_index {
                offset -= 1;
                self.cursor = 0;
            }

            if offset < last_index - self.cursor {
                self.cursor += offset;
            } else {
                self.cursor = last_index;
            }
        } else if offset < 0 {
            let mut offset = (-offset) as usize;
            if self.cursor == 0 {
                offset -= 1;
                self.cursor = last_index;
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

    pub fn clear_filtered(&mut self) {
        self.cursor = 0;
        self.scroll = 0;

        self.filtered.clear();
        self.filter.clear();
    }

    pub fn add_provider(&mut self, provider: Box<dyn SelectEntryProvider>) {
        self.providers.push(provider);
    }

    pub fn clear_providers(&mut self) {
        self.providers.clear();
    }

    pub fn set_filter(&mut self, filter: &str) {
        self.filter.clear();
        self.filter.push_str(filter);
        self.filter();
    }

    fn filter(&mut self) {
        self.filtered.clear();
        let filter = &self.filter[..];
        for (pi, p) in self.providers.iter().enumerate() {
            for (ei, e) in p.provide_entries().iter().enumerate() {
                if let Some(score) = self.matcher.fuzzy_match(&e.name[..], filter) {
                    self.filtered.push(FilteredEntry {
                        provider_index: pi,
                        entry_index: ei,
                        score,
                    });
                }
            }
        }

        self.filtered.sort_unstable_by(|a, b| b.score.cmp(&a.score));
        self.cursor = self.cursor.min(self.filtered.len());
        self.move_cursor(0);
    }

    pub fn filtered_entries(&self) -> impl Iterator<Item = &SelectEntry> {
        self.filtered
            .iter()
            .map(move |f| &self.providers[f.provider_index].provide_entries()[f.entry_index])
    }
}
