#[derive(Default)]
pub struct SelectEntry {
    pub name: String,
}

#[derive(Default)]
pub struct SelectEntryCollection {
    pub selected_index: usize,
    len: usize,
    entries: Vec<SelectEntry>,
}

impl SelectEntryCollection {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn clear(&mut self) {
        self.selected_index = 0;
        self.len = 0;
    }

    pub fn add(&mut self, name: &str) {
        let entry = if self.len < self.entries.len() {
            &mut self.entries[self.len]
        } else {
            self.entries.push(SelectEntry::default());
            self.len = self.entries.len();
            &mut self.entries[self.len - 1]
        };

        entry.name.clear();
        entry.name.push_str(name);
    }

    pub fn entries_from(&self, index: usize) -> impl Iterator<Item = &SelectEntry> {
        if index < self.len {
            self.entries[index..self.len].iter()
        } else {
            self.entries[..0].iter()
        }
    }
}
