use std::{cmp::Ordering, io::Write, iter};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal, ErrorKind, Result,
};

use crate::{
    application::UI,
    buffer_position::BufferPosition,
    client::Client,
    event::{Event, Key},
    mode::Mode,
    theme,
    theme::Theme,
};

pub fn convert_event(event: event::Event) -> Event {
    match event {
        event::Event::Key(e) => match e.code {
            event::KeyCode::Backspace => Event::Key(Key::Backspace),
            event::KeyCode::Enter => Event::Key(Key::Enter),
            event::KeyCode::Left => Event::Key(Key::Left),
            event::KeyCode::Right => Event::Key(Key::Right),
            event::KeyCode::Up => Event::Key(Key::Up),
            event::KeyCode::Down => Event::Key(Key::Down),
            event::KeyCode::Home => Event::Key(Key::Home),
            event::KeyCode::End => Event::Key(Key::End),
            event::KeyCode::PageUp => Event::Key(Key::PageUp),
            event::KeyCode::PageDown => Event::Key(Key::PageDown),
            event::KeyCode::Tab => Event::Key(Key::Tab),
            event::KeyCode::Delete => Event::Key(Key::Delete),
            event::KeyCode::F(f) => Event::Key(Key::F(f)),
            event::KeyCode::Char(c) => match e.modifiers {
                event::KeyModifiers::CONTROL => Event::Key(Key::Ctrl(c)),
                event::KeyModifiers::ALT => Event::Key(Key::Alt(c)),
                _ => Event::Key(Key::Char(c)),
            },
            event::KeyCode::Esc => Event::Key(Key::Esc),
            _ => Event::None,
        },
        event::Event::Resize(w, h) => Event::Resize(w, h),
        _ => Event::None,
    }
}

pub const fn convert_color(color: theme::Color) -> Color {
    Color::Rgb {
        r: color.0,
        g: color.1,
        b: color.2,
    }
}

pub struct Tui<W>
where
    W: Write,
{
    write: W,
}

impl<W> Tui<W>
where
    W: Write,
{
    pub fn new(write: W) -> Self {
        Self { write }
    }
}

impl<W> UI for Tui<W>
where
    W: Write,
{
    type Error = ErrorKind;

    fn init(&mut self) -> Result<()> {
        handle_command!(self.write, terminal::EnterAlternateScreen)?;
        self.write.flush()?;
        handle_command!(self.write, cursor::Hide)?;
        self.write.flush()?;
        terminal::enable_raw_mode()?;
        Ok(())
    }

    fn draw(
        &mut self,
        client: &Client,
        width: u16,
        height: u16,
        error: Option<String>,
    ) -> Result<()> {
        draw(&mut self.write, client, width, height, error)
    }

    fn shutdown(&mut self) -> Result<()> {
        handle_command!(self.write, terminal::LeaveAlternateScreen)?;
        handle_command!(self.write, cursor::Show)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }
}

fn draw<W>(
    write: &mut W,
    client: &Client,
    width: u16,
    height: u16,
    error: Option<String>,
) -> Result<()>
where
    W: Write,
{
    enum DrawState {
        Normal,
        Selection,
        Highlight,
        Cursor,
    }

    //handle_command!(write, SetBackgroundColor(Color::Red))?;
    //handle_command!(write, terminal::Clear(terminal::ClearType::All))?;

    let theme = &client.config.theme;

    handle_command!(write, cursor::Hide)?;

    let cursor_color = match client.mode {
        Mode::Select => convert_color(theme.cursor_select),
        Mode::Insert => convert_color(theme.cursor_insert),
        _ => convert_color(theme.cursor_normal),
    };

    handle_command!(write, cursor::MoveTo(0, 0))?;
    handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
    handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;

    let mut line_index = client.scroll;
    let mut drawn_line_count = 0;

    'lines_loop: for line in client.buffer.lines_from(line_index) {
        let mut draw_state = DrawState::Normal;
        let mut column_index = 0;
        let mut x = 0;

        for c in line.text.chars().chain(iter::once(' ')) {
            if x >= width {
                handle_command!(write, cursor::MoveToNextLine(1))?;

                draw_state = DrawState::Normal;
                drawn_line_count += 1;
                x = 0;

                if drawn_line_count == height {
                    break 'lines_loop;
                }
            }

            let char_position = BufferPosition::line_col(line_index, column_index);
            if client.cursors[..]
                .binary_search_by_key(&char_position, |c| c.position)
                .is_ok()
            {
                if !matches!(draw_state, DrawState::Cursor) {
                    draw_state = DrawState::Cursor;
                    handle_command!(write, SetBackgroundColor(cursor_color))?;
                    handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;
                }
            } else if client.cursors[..]
                .binary_search_by(|c| {
                    let range = c.range();
                    if range.to < char_position {
                        Ordering::Less
                    } else if range.from > char_position {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                })
                .is_ok()
            {
                if !matches!(draw_state, DrawState::Selection) {
                    draw_state = DrawState::Selection;
                    handle_command!(write, SetBackgroundColor(convert_color(theme.text_normal)))?;
                    handle_command!(write, SetForegroundColor(convert_color(theme.background)))?;
                }
            } else if client
                .search_ranges
                .binary_search_by(|r| {
                    if r.to < char_position {
                        Ordering::Less
                    } else if r.from > char_position {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                })
                .is_ok()
            {
                if !matches!(draw_state, DrawState::Highlight) {
                    draw_state = DrawState::Highlight;
                    handle_command!(write, SetBackgroundColor(convert_color(theme.highlight)))?;
                    handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;
                }
            } else if !matches!(draw_state, DrawState::Normal) {
                draw_state = DrawState::Normal;
                handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
                handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;
            }

            match c {
                '\t' => {
                    for _ in 0..client.config.tab_size {
                        handle_command!(write, Print(' '))?
                    }
                    column_index += client.config.tab_size;
                    x += client.config.tab_size as u16;
                }
                _ => {
                    handle_command!(write, Print(c))?;
                    column_index += 1;
                    x += 1;
                }
            }
        }

        handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
        handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;
        for _ in x..width {
            handle_command!(write, Print(' '))?;
        }
        handle_command!(write, cursor::MoveToNextLine(1))?;

        line_index += 1;
        drawn_line_count += 1;

        if drawn_line_count == height {
            break;
        }
    }

    handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
    handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;
    for _ in drawn_line_count..height {
        handle_command!(write, Print('~'))?;
        for _ in 0..(width - 1) {
            handle_command!(write, Print(' '))?;
        }
        handle_command!(write, cursor::MoveToNextLine(1))?;
    }

    if client.has_focus {
        handle_command!(write, SetBackgroundColor(convert_color(theme.text_normal)))?;
        handle_command!(write, SetForegroundColor(convert_color(theme.background)))?;
    } else {
        handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
        handle_command!(
            write,
            SetForegroundColor(convert_color(theme.text_normal))
        )?;
    }
    let buffer_name = "the buffer name";
    handle_command!(write, Print(buffer_name))?;
    for _ in buffer_name.len()..(width as usize - 1) {
        handle_command!(write, Print(' '))?;
    }

    handle_command!(write, ResetColor)?;

    handle_command!(write, cursor::MoveToNextLine(1))?;
    draw_statusbar(write, client, error)?;

    write.flush()?;
    Ok(())
}

fn draw_statusbar<W>(write: &mut W, client: &Client, error: Option<String>) -> Result<()>
where
    W: Write,
{
    fn draw_input<W>(write: &mut W, prefix: &str, input: &str, theme: &Theme) -> Result<()>
    where
        W: Write,
    {
        handle_command!(write, Print(prefix))?;
        handle_command!(write, Print(input))?;
        handle_command!(
            write,
            SetBackgroundColor(convert_color(theme.cursor_normal))
        )?;
        handle_command!(write, Print(' '))?;
        handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
        Ok(())
    }

    handle_command!(
        write,
        SetBackgroundColor(convert_color(client.config.theme.background))
    )?;
    handle_command!(
        write,
        SetForegroundColor(convert_color(client.config.theme.text_normal))
    )?;

    if let Some(error) = error {
        handle_command!(write, Print("error: "))?;
        handle_command!(write, Print(error))?;
    } else {
        match client.mode {
            Mode::Select => handle_command!(write, Print("-- SELECT --"))?,
            Mode::Insert => handle_command!(write, Print("-- INSERT --"))?,
            Mode::Search(_) => {
                draw_input(write, "search: ", &client.input[..], &client.config.theme)?
            }
            Mode::Command(_) => {
                draw_input(write, "command: ", &client.input[..], &client.config.theme)?
            }
            _ => (),
        };
    }

    handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
    Ok(())
}
