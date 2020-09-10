use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::{buffer::BufferCollection, buffer_view::BufferViewCollection};

pub struct SelectContext<'a> {
    pub buffers: &'a BufferCollection,
    pub buffer_views: &'a BufferViewCollection,
}

pub trait SelectSource {
    fn len(&self) -> usize;
    fn entry(&self, index: usize) -> SelectEntry;
}

impl<'a> SelectSource for SelectEntry<'a> {
    fn len(&self) -> usize {
        1
    }

    fn entry(&self, _index: usize) -> SelectEntry {
        *self
    }
}

impl<'a> SelectSource for &[SelectEntry<'a>] {
    fn len(&self) -> usize {
        <[SelectEntry]>::len(self)
    }

    fn entry(&self, index: usize) -> SelectEntry {
        self[index]
    }
}

#[derive(Default, Clone, Copy)]
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

struct SelectEntryInternal {
    pub source_index: usize,
    pub entry_index: usize,
    pub score: i64,
}

type SourceSelector = for<'a> fn(&SelectContext<'a>) -> &'a dyn SelectSource;

#[derive(Default)]
pub struct SelectEntryCollection {
    matcher: SkimMatcherV2,
    source_selectors: &'static [SourceSelector],
    entries: Vec<SelectEntryInternal>,

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
        self.entries.len().min(max_height)
    }

    pub fn move_cursor(&mut self, offset: isize) {
        if self.entries.len() == 0 {
            return;
        }

        let end_index = self.entries.len() - 1;

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

    pub fn set_sources(&mut self, selectors: &'static [SourceSelector]) {
        self.source_selectors = selectors;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    pub fn filter(&mut self, ctx: &SelectContext, pattern: &str) {
        self.entries.clear();

        for (source_index, selector) in self.source_selectors.iter().enumerate() {
            let source = selector(ctx);
            for entry_index in 0..source.len() {
                let entry = source.entry(entry_index);
                if let Some(score) = self.matcher.fuzzy_match(entry.name, pattern) {
                    self.entries.push(SelectEntryInternal {
                        source_index,
                        entry_index,
                        score,
                    });
                }
            }
        }

        self.entries.sort_unstable_by(|a, b| b.score.cmp(&a.score));
        self.cursor = self.cursor.min(self.entries.len());
    }

    pub fn entry<'a>(&self, ctx: &SelectContext<'a>, index: usize) -> SelectEntry<'a> {
        let entry = &self.entries[index];
        let selector = self.source_selectors[entry.source_index];
        selector(ctx).entry(entry.entry_index)
    }

    pub fn entries<'a>(
        &'a self,
        ctx: &'a SelectContext<'a>,
    ) -> impl 'a + Iterator<Item = SelectEntry<'a>> {
        self.entries.iter().map(move |e| {
            let selector = self.source_selectors[e.source_index];
            selector(ctx).entry(e.entry_index)
        })
    }
}
