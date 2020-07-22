use std::{cmp::Ordering, io::Write, iter, sync::mpsc, thread};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal, ErrorKind, Result,
};

use crate::{
    application::{UiError, UI},
    buffer_position::BufferPosition,
    client::Client,
    event::{Event, Key},
    mode::Mode,
    theme,
};

fn convert_event(event: event::Event) -> Event {
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

const fn convert_color(color: theme::Color) -> Color {
    Color::Rgb {
        r: color.0,
        g: color.1,
        b: color.2,
    }
}

impl UiError for ErrorKind {}

pub struct Tui<W>
where
    W: Write,
{
    write: W,
    scroll: usize,
    width: u16,
    height: u16,
}

impl<W> Tui<W>
where
    W: Write,
{
    pub fn new(write: W) -> Self {
        Self {
            write,
            scroll: 0,
            width: 0,
            height: 0,
        }
    }
}

impl<W> UI for Tui<W>
where
    W: Write,
{
    type Error = ErrorKind;

    fn run_event_loop_in_background(event_sender: mpsc::Sender<Event>) -> thread::JoinHandle<Result<()>> {
        thread::spawn(move || {
            while event_sender.send(convert_event(event::read()?)).is_ok() {}
            Ok(())
        })
    }

    fn init(&mut self) -> Result<()> {
        handle_command!(self.write, terminal::EnterAlternateScreen)?;
        self.write.flush()?;
        handle_command!(self.write, cursor::Hide)?;
        self.write.flush()?;
        terminal::enable_raw_mode()?;

        let size = terminal::size()?;
        self.resize(size.0, size.1)
    }

    fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn draw(&mut self, client: &Client, error: Option<String>) -> Result<()> {
        let cursor_position = client.main_cursor.position;
        if cursor_position.line_index < self.scroll {
            self.scroll = cursor_position.line_index;
        } else if cursor_position.line_index >= self.scroll + self.height as usize {
            self.scroll = cursor_position.line_index - self.height as usize + 1;
        }

        draw(
            &mut self.write,
            client,
            self.scroll,
            self.width,
            self.height,
            error,
        )
    }

    fn shutdown(&mut self) -> Result<()> {
        handle_command!(self.write, ResetColor)?;
        handle_command!(
            self.write,
            terminal::Clear(terminal::ClearType::UntilNewLine)
        )?;
        handle_command!(self.write, terminal::LeaveAlternateScreen)?;
        handle_command!(self.write, cursor::Show)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }
}

fn draw<W>(
    write: &mut W,
    client: &Client,
    scroll: usize,
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

    let mut line_index = scroll;
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

                if drawn_line_count >= height - 1 {
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
        handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;

        line_index += 1;
        drawn_line_count += 1;

        if drawn_line_count >= height - 1 {
            break;
        }
    }

    handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
    handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;
    for _ in drawn_line_count..(height - 1) {
        handle_command!(write, Print('~'))?;
        handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;
    }

    if client.has_focus {
        handle_command!(write, SetBackgroundColor(convert_color(theme.text_normal)))?;
        handle_command!(write, SetForegroundColor(convert_color(theme.background)))?;
    } else {
        handle_command!(write, SetBackgroundColor(convert_color(theme.background)))?;
        handle_command!(write, SetForegroundColor(convert_color(theme.text_normal)))?;
    }

    handle_command!(write, cursor::MoveToNextLine(1))?;
    draw_statusbar(write, client, error)?;

    write.flush()?;
    Ok(())
}

fn draw_statusbar<W>(write: &mut W, client: &Client, error: Option<String>) -> Result<()>
where
    W: Write,
{
    fn draw_input<W>(
        write: &mut W,
        prefix: &str,
        input: &str,
        background_color: Color,
        cursor_color: Color,
    ) -> Result<()>
    where
        W: Write,
    {
        handle_command!(write, Print(prefix))?;
        handle_command!(write, Print(input))?;
        handle_command!(write, SetBackgroundColor(cursor_color))?;
        handle_command!(write, Print(' '))?;
        handle_command!(write, SetBackgroundColor(background_color))?;
        Ok(())
    }

    let background_color = convert_color(client.config.theme.text_normal);
    let foreground_color = convert_color(client.config.theme.background);
    let cursor_color = convert_color(client.config.theme.cursor_normal);

    if !client.has_focus {
        handle_command!(write, SetBackgroundColor(foreground_color))?;
        handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;

        if let Some(error) = error {
            handle_command!(write, SetForegroundColor(background_color))?;
            handle_command!(write, Print("error:"))?;
            handle_command!(write, Print(error))?;
        }
        return Ok(());
    }

    handle_command!(write, SetBackgroundColor(background_color))?;
    handle_command!(write, SetForegroundColor(foreground_color))?;

    if let Some(error) = error {
        handle_command!(write, Print("error:"))?;
        handle_command!(write, Print(error))?;
    } else {
        match client.mode {
            Mode::Select => handle_command!(write, Print("-- SELECT --"))?,
            Mode::Insert => handle_command!(write, Print("-- INSERT --"))?,
            Mode::Search(_) => draw_input(
                write,
                "search:",
                &client.input[..],
                background_color,
                cursor_color,
            )?,
            Mode::Command(_) => draw_input(
                write,
                "command:",
                &client.input[..],
                background_color,
                cursor_color,
            )?,
            _ => (),
        };
    }

    handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
    Ok(())
}
