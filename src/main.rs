use std::io::{stdout, Write};

use crossterm::{
    cursor, queue,
    style::{Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
    Result,
};

use ropey;
use syntect::{easy, highlighting, parsing};

fn main() -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;

    let stdout = stdout();
    let mut stdout = stdout.lock();

    let src = include_str!("main.rs");
    let rope = ropey::Rope::from_str(src);

    let syntax_set = parsing::SyntaxSet::load_defaults_newlines();
    let theme_set = highlighting::ThemeSet::load_defaults();
    let syntax = syntax_set.find_syntax_by_extension("rs").unwrap();
    let mut h = easy::HighlightLines::new(syntax, &theme_set.themes["base16-ocean.dark"]);

    queue!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0))?;

    for line in rope.lines() {
        let ranges: Vec<(highlighting::Style, &str)> =
            h.highlight(line.as_str().unwrap_or(""), &syntax_set);
        let escaped = syntect::util::as_24_bit_terminal_escaped(&ranges[..], true);
        queue!(stdout, Print(escaped))?;
    }

    queue!(stdout, ResetColor)?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}
