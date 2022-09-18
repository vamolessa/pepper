use std::{
    io,
    sync::atomic::{AtomicPtr, Ordering},
};

use crate::ResourceFile;

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
        name: "help.md",
        content: include_str!("../rc/help.md"),
    },
    ResourceFile {
        name: "changelog.md",
        content: include_str!("../rc/changelog.md"),
    },
    ResourceFile {
        name: "command_reference.md",
        content: include_str!("../rc/command_reference.md"),
    },
    ResourceFile {
        name: "expansion_reference.md",
        content: include_str!("../rc/expansion_reference.md"),
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
    crate::DEFAULT_CONFIGS,
    crate::DEFAULT_SYNTAXES,
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

pub(crate) fn help_page_names() -> impl Iterator<Item = &'static str> {
    HelpPageIterator::new().map(|r| r.name)
}

#[derive(Default)]
pub(crate) struct HelpPageName<'a>(&'a str);
impl<'a> HelpPageName<'a> {
    pub fn as_str(&self) -> &'a str {
        self.0
    }
}

pub(crate) fn parse_help_page_name(page_name: &str) -> Option<HelpPageName> {
    let page_name = page_name.strip_prefix(HELP_PREFIX)?;
    Some(HelpPageName(page_name))
}

pub(crate) fn open(page_name: HelpPageName) -> impl io::BufRead {
    let page_name = page_name.0;
    for page in HelpPageIterator::new() {
        if page_name == page.name {
            return io::Cursor::new(page.content);
        }
    }
    let main_page_content = MAIN_HELP_PAGES.pages[0].content;
    io::Cursor::new(main_page_content)
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
