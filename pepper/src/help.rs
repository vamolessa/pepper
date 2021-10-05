use std::{io, path::Path};

use crate::ResourceFile;

pub const HELP_PREFIX: &str = "help://";

static HELP_FILES: &[ResourceFile] = &[
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
        name: "help.md",
        content: include_str!("../rc/help.md"),
    },
];

pub fn main_help_name() -> &'static str {
    HELP_FILES[HELP_FILES.len() - 1].name
}

pub fn open(path: &Path) -> Option<impl io::BufRead> {
    let path = match path.to_str().and_then(|p| p.strip_prefix(HELP_PREFIX)) {
        Some(path) => path,
        None => return None,
    };
    for file in HELP_FILES {
        if path == file.name {
            return Some(io::Cursor::new(file.content));
        }
    }
    None
}

pub fn search(keyword: &str) -> Option<(&'static str, usize)> {
    let mut last_match = None;
    for file in HELP_FILES {
        if keyword == file.name.trim_end_matches(".md") {
            return Some((file.name, 0));
        }

        for (line_index, line) in file.content.lines().enumerate() {
            if line.contains(keyword) {
                if line.starts_with('#') {
                    return Some((file.name, line_index));
                } else {
                    last_match = Some((file.name, line_index));
                }
            }
        }
    }
    last_match
}

