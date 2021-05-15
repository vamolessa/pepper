use std::{io, path::Path};

use crate::word_database::{WordIter, WordKind};

pub static HELP_PREFIX: &str = "help://";

pub static HELP_SOURCES: &[(&str, &str)] = &[
    ("help://index.md", include_str!("../rc/index.md")),
    ("help://bindings.md", include_str!("../rc/bindings.md")),
    ("help://command_reference.md", include_str!("../rc/command_reference.md")),
    ("help://language_syntax_definitions.md", include_str!("../rc/language_syntax_definitions.md")),
    ("help://config_recipes.md", include_str!("../rc/config_recipes.md")),
];

pub fn open(path: &Path) -> Option<impl io::BufRead> {
    let path = match path.to_str().and_then(|p| p.strip_prefix(HELP_PREFIX)) {
        Some(path) => path,
        None => return None,
    };
    for &(help_path, help_source) in HELP_SOURCES {
        if path == &help_path[HELP_PREFIX.len()..] {
            return Some(io::Cursor::new(help_source));
        }
    }
    None
}

pub fn search(keyword: &str) -> Option<(&'static Path, usize)> {
    let mut best_match = None;
    for &(path, source) in HELP_SOURCES {
        for (line_index, line) in source.lines().enumerate() {
            for word in WordIter(line).of_kind(WordKind::Identifier) {
                if word == keyword {
                    let path = Path::new(path);
                    if line.starts_with('#') {
                        return Some((path, line_index));
                    } else {
                        best_match = Some((path, line_index));
                        break;
                    }
                }
            }
        }
    }
    best_match
}
