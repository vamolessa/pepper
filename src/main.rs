use std::{
    io::{stdout, Write},
    iter,
};

use crossterm::{
    cursor,
    handle_command,
    style::{Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    // terminal::{self, Clear, ClearType},
    Result,
};

use ropey;
use syntect::{easy, highlighting, parsing};

mod buffer;

fn to_crossterm_color(c: highlighting::Color) -> crossterm::style::Color {
    crossterm::style::Color::Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

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

	let mut highlighter = 

    for line in rope.lines() {
        let line_str = line
            .chars()
            // .flat_map(|c| {
            //     if c == '\t' {
            //         iter::repeat(' ').take(4)
            //     } else if c == '\n' {
            //         iter::repeat(' ').take(1)
            //     } else {
            //         iter::repeat(c).take(1)
            //     }
            // })
            // .chain(iter::once('\n'))
            .collect::<String>();

        let ranges: Vec<(highlighting::Style, &str)> = h.highlight(&line_str, &syntax_set);
        for (style, slice) in ranges {
            handle_command!(
                stdout,
                SetForegroundColor(to_crossterm_color(style.foreground))
            )?;
            handle_command!(
                stdout,
                SetBackgroundColor(to_crossterm_color(style.background))
            )?;
            handle_command!(stdout, Print(slice))?;
        }
        handle_command!(stdout, cursor::MoveToNextLine(1))?;

        // handle_command!(stdout, Print(line_str))?;
        // handle_command!(stdout, cursor::MoveToNextLine(1))?;
    }

    handle_command!(stdout, ResetColor)?;
    handle_command!(stdout, cursor::MoveToNextLine(1))?;
    stdout.flush()?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}
