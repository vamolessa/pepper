use std::{
    io,
    path::Path,
    sync::atomic::{AtomicPtr, Ordering},
};

use crate::{buffer_position::BufferPosition, ResourceFile};

pub const HELP_PREFIX: &str = "help://";

struct HelpPages {
    pages: &'static [ResourceFile],
    next: AtomicPtr<HelpPages>,
}
impl HelpPages {
    pub const fn new(pages: &'static [ResourceFile]) -> Self {
        Self {
            pages,
            next: AtomicPtr::new(std::ptr::null_mut()),
        }
    }
}

static MAIN_HELP_PAGES: HelpPages = HelpPages::new(&[
    ResourceFile {
        name: "changelog.md",
        content: include_str!("../rc/changelog.md"),
    },
    ResourceFile {
        name: "command_reference.md",
        content: include_str!("../rc/command_reference.md"),
    },
    ResourceFile {
        name: "bindings.md",
        content: include_str!("../rc/bindings.md"),
    },
    ResourceFile {
        name: "language_syntax_definitions.md",
        content: include_str!("../rc/language_syntax_definitions.md"),
    },
    ResourceFile {
        name: "config_recipes.md",
        content: include_str!("../rc/config_recipes.md"),
    },
    ResourceFile {
        name: "default_configs.pepper",
        content: include_str!("../rc/default_configs.pepper"),
    },
    ResourceFile {
        name: "default_syntaxes.pepper",
        content: include_str!("../rc/default_syntaxes.pepper"),
    },
    ResourceFile {
        name: "help.md",
        content: include_str!("../rc/help.md"),
    },
]);

pub(crate) fn add_help_pages(pages: &'static [ResourceFile]) {
    if pages.is_empty() {
        return;
    }

    let pages = Box::into_raw(Box::new(HelpPages::new(pages)));
    let mut current = &MAIN_HELP_PAGES;
    loop {
        match current.next.compare_exchange(
            std::ptr::null_mut(),
            pages,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => current = unsafe { &*next },
        }
    }
}

pub(crate) fn main_help_name() -> &'static str {
    MAIN_HELP_PAGES.pages[MAIN_HELP_PAGES.pages.len() - 1].name
}

pub(crate) fn open(path: &Path) -> Option<impl io::BufRead> {
    let path = path.to_str()?;
    let path = match path.strip_prefix(HELP_PREFIX) {
        Some(path) => path,
        None => path,
    };
    for page in HelpPageIterator::new() {
        if path == page.name {
            return Some(io::Cursor::new(page.content));
        }
    }
    None
}

pub(crate) fn search(keyword: &str) -> Option<(&'static str, BufferPosition)> {
    let mut last_match = None;
    for page in HelpPageIterator::new() {
        let page_name = match page.name.strip_suffix(".md") {
            Some(name) => name,
            None => page.name,
        };

        if keyword == page_name {
            return Some((page.name, BufferPosition::zero()));
        }

        for (line_index, line) in page.content.lines().enumerate() {
            if let Some(column_index) = line.find(keyword) {
                let position = BufferPosition::line_col(line_index as _, column_index as _);
                if line.starts_with('#') {
                    return Some((page.name, position));
                } else {
                    last_match = Some((page.name, position));
                }
            }
        }
    }

    last_match
}

struct HelpPageIterator {
    current: &'static HelpPages,
    index: usize,
}
impl HelpPageIterator {
    pub fn new() -> Self {
        Self {
            current: &MAIN_HELP_PAGES,
            index: 0,
        }
    }
}
impl Iterator for HelpPageIterator {
    type Item = ResourceFile;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.index < self.current.pages.len() {
                let page = self.current.pages[self.index];
                self.index += 1;
                break Some(page);
            } else {
                let next = self.current.next.load(Ordering::Relaxed);
                if next.is_null() {
                    break None;
                } else {
                    self.current = unsafe { &*next };
                    self.index = 0;
                }
            }
        }
    }
}
