use std::io::{stdout, Write};

use crossterm::{
    cursor, handle_command,
    style::{Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
    Result,
};

use ropey;
use syntect::{
    highlighting::{self, HighlightIterator, HighlightState, Highlighter, Theme},
    parsing::{self, ParseState, ScopeStack, SyntaxReference},
};

mod buffer;

fn to_crossterm_color(c: highlighting::Color) -> crossterm::style::Color {
    crossterm::style::Color::Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

pub struct HighlightLines<'a> {
    pub highlighter: Highlighter<'a>,
    pub parse_state: ParseState,
    pub highlight_state: HighlightState,
}

impl<'a> HighlightLines<'a> {
    pub fn new(syntax: &SyntaxReference, theme: &'a Theme) -> Self {
        let highlighter = Highlighter::new(theme);
        let highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
        HighlightLines {
            highlighter,
            parse_state: ParseState::new(syntax),
            highlight_state,
        }
    }
}

fn main() -> Result<()> {
    terminal::enable_raw_mode()?;

    let stdout = stdout();
    let mut stdout = stdout.lock();
    let terminal_size = terminal::size()?;

    let src = include_str!("main.rs");
    let rope = ropey::Rope::from_str(src);

    let syntax_set = parsing::SyntaxSet::load_defaults_newlines();
    let theme_set = highlighting::ThemeSet::load_defaults();
    let syntax = syntax_set.find_syntax_by_extension("rs").unwrap();

    let mut h = HighlightLines::new(&syntax, &theme_set.themes["base16-mocha.dark"]);

    let mut highlighted_lines = Vec::new();
    for line in rope.lines() {
        use std::fmt::Write;
        let line: String = line.chars().collect();
        let mut highlighted_line = String::new();

        // handle_command!(highlighted_line, Print('~'))?;

        let ops = h.parse_state.parse_line(&line[..], &syntax_set);
        for (style, slice) in
            HighlightIterator::new(&mut h.highlight_state, &ops[..], &line[..], &h.highlighter)
        {
            handle_command!(
                highlighted_line,
                SetForegroundColor(to_crossterm_color(style.foreground))
            )?;
            handle_command!(
                highlighted_line,
                SetBackgroundColor(to_crossterm_color(style.background))
            )?;
            handle_command!(highlighted_line, Print(slice))?;

            // for line in slice.lines() {
            //     handle_command!(highlighted_line, Print(line))?;
            //     handle_command!(highlighted_line, Clear(ClearType::UntilNewLine))?;
            //     // handle_command!(highlighted_line, cursor::MoveToNextLine(1))?;
            //     handle_command!(highlighted_line, Print('\n'))?;
            // }
        }
        highlighted_lines.push(highlighted_line);
    }

    for line in &highlighted_lines {
        handle_command!(stdout, Print(line))?;
    }

    handle_command!(stdout, ResetColor)?;
    handle_command!(stdout, cursor::MoveToNextLine(1))?;
    stdout.flush()?;
    terminal::disable_raw_mode()?;
    Ok(())
}
