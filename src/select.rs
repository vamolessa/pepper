use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

pub trait SelectSource {
    fn entries(&self) -> &[SelectEntryRef];
}

impl<'a> SelectSource for &[SelectEntryRef<'a>] {
    fn entries(&self) -> &[SelectEntryRef] {
        self
    }
}

#[derive(Default, Clone, Copy)]
pub struct SelectEntryRef<'a> {
    pub name: &'a str,
    pub description: &'a str,
}

impl<'a> SelectEntryRef<'a> {
    pub const fn from_str(name: &'a str) -> Self {
        Self {
            name,
            description: "",
        }
    }
}

#[derive(Default)]
pub struct SelectEntry {
    pub name: String,
    pub description: String,
    pub score: i64,
}

#[derive(Default)]
pub struct SelectEntryCollection {
    matcher: SkimMatcherV2,
    len: usize,
    entries: Vec<SelectEntry>,
    pattern: String,

    cursor: usize,
    scroll: usize,
}

impl SelectEntryCollection {
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn height(&self, max_height: usize) -> usize {
        self.len.min(max_height)
    }

    pub fn pattern(&self) -> &str {
        &self.pattern[..]
    }

    pub fn move_cursor(&mut self, offset: isize) {
        if self.len == 0 {
            return;
        }

        let last_index = self.len - 1;
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

    pub fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
        self.scroll = 0;
    }

    pub fn filter(&mut self, sources: &[&dyn SelectSource], pattern: &str) {
        self.len = 0;

        for s in sources {
            for e in s.entries() {
                if let Some(score) = self.matcher.fuzzy_match(e.name, pattern) {
                    if self.len == self.entries.len() {
                        self.entries.push(SelectEntry::default());
                    }

                    let entry = &mut self.entries[self.len];
                    entry.name.clear();
                    entry.name.push_str(e.name);
                    entry.description.clear();
                    entry.description.push_str(e.description);
                    entry.score = score;

                    self.len += 1;
                }
            }
        }

        self.pattern.clear();
        self.pattern.push_str(pattern);

        self.entries.sort_unstable_by(|a, b| b.score.cmp(&a.score));
        self.cursor = self.cursor.min(self.len);
    }

    pub fn entries(&self) -> impl Iterator<Item = &SelectEntry> {
        self.entries.iter()
    }
}
