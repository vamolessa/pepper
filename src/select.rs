use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

pub trait SelectSource {
    fn entries(&self) -> &[SelectEntryRef];
}

impl<'a> SelectSource for SelectEntryRef<'a> {
    fn entries(&self) -> &[SelectEntryRef] {
        std::slice::from_ref(self)
    }
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

    cursor: Option<usize>,
    scroll: usize,
}

impl SelectEntryCollection {
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn height(&self, max_height: usize) -> usize {
        self.len.min(max_height)
    }

    pub fn move_cursor(&mut self, offset: isize) {
        if self.len == 0 {
            return;
        }

        let end_index = self.len - 1;
        let mut cursor = 0;

        if offset > 0 {
            cursor = self.cursor.unwrap_or(end_index);

            let mut offset = offset as usize;
            if cursor == end_index {
                offset -= 1;
                cursor = 0;
            }

            if offset < end_index - cursor {
                cursor += offset;
            } else {
                cursor = end_index;
            }
        } else if offset < 0 {
            cursor = self.cursor.unwrap_or(0);

            let mut offset = (-offset) as usize;
            if cursor == 0 {
                offset -= 1;
                cursor = end_index;
            }

            if offset < cursor {
                cursor -= offset;
            } else {
                cursor = 0;
            }
        }

        self.cursor = Some(cursor);
    }

    pub fn update_scroll(&mut self, max_height: usize) {
        let cursor = self.cursor.unwrap_or(0);
        let height = self.height(max_height);
        if cursor < self.scroll {
            self.scroll = cursor;
        } else if cursor >= self.scroll + height as usize {
            self.scroll = cursor + 1 - height as usize;
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.cursor = None;
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

        self.entries.sort_unstable_by(|a, b| b.score.cmp(&a.score));
        self.cursor = self.cursor.map(|c| c.min(self.len));
    }

    pub fn selected_entry(&self) -> Option<&SelectEntry> {
        self.cursor.map(|c| &self.entries[c])
    }

    pub fn entries(&self) -> impl Iterator<Item = &SelectEntry> {
        self.entries.iter()
    }
}
