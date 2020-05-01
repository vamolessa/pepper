use std::{io::Write, iter};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
    Result,
};

use crate::{
    editor::Editor,
    event::{Event, Key},
    theme,
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
            event::KeyCode::BackTab => Event::Key(Key::BackTab),
            event::KeyCode::Delete => Event::Key(Key::Delete),
            event::KeyCode::Insert => Event::Key(Key::Insert),
            event::KeyCode::F(f) => Event::Key(Key::F(f)),
            event::KeyCode::Char(c) => match e.modifiers {
                event::KeyModifiers::CONTROL => Event::Key(Key::Ctrl(c)),
                event::KeyModifiers::ALT => Event::Key(Key::Alt(c)),
                _ => Event::Key(Key::Char(c)),
            },
            event::KeyCode::Null => Event::Key(Key::Null),
            event::KeyCode::Esc => Event::Key(Key::Esc),
        },
        event::Event::Resize(w, h) => Event::Resize(w, h),
        _ => Event::None,
    }
}

pub fn convert_color(color: theme::Color) -> Color {
    Color::Rgb {
        r: color.0,
        g: color.1,
        b: color.2,
    }
}

fn update_buffer_views_size(editor: &mut Editor) {
    let size = terminal::size().unwrap_or((0, 0));
    editor.set_view_size(size);
}

pub fn show<W>(mut write: W, mut editor: Editor) -> Result<()>
where
    W: Write,
{
    handle_command!(write, terminal::EnterAlternateScreen)?;
    write.flush()?;
    handle_command!(write, cursor::Hide)?;
    write.flush()?;
    terminal::enable_raw_mode()?;

    update_buffer_views_size(&mut editor);
    draw(&mut write, &editor, 0)?;

    loop {
        let event = convert_event(event::read()?);
        if editor.on_event(&event) {
            break;
        }
        draw(&mut write, &editor, 0)?;
    }

    handle_command!(write, terminal::LeaveAlternateScreen)?;
    handle_command!(write, cursor::Show)?;
    terminal::disable_raw_mode().unwrap();

    Ok(())
}

fn draw<W>(write: &mut W, editor: &Editor, view_index: usize) -> Result<()>
where
    W: Write,
{
    let buffer_view = &editor.buffer_views[view_index];
    let buffer = &editor.buffers[buffer_view.buffer_handle];

    handle_command!(write, cursor::MoveTo(0, 0))?;
    handle_command!(write, cursor::Hide)?;

    handle_command!(
        write,
        SetForegroundColor(convert_color(editor.theme.foreground))
    )?;
    handle_command!(
        write,
        SetBackgroundColor(convert_color(editor.theme.background))
    )?;

    let mut was_inside_selection = false;
    for (y, line) in buffer
        .lines
        .iter()
        .skip(buffer_view.scroll as usize)
        .take(buffer_view.size.1 as usize)
        .enumerate()
    {
        let y = y + buffer_view.scroll as usize;
        for (x, c) in line.chars().chain(iter::once(' ')).enumerate() {
            let inside_selection = x == buffer_view.cursor.column_index as usize
                && y == buffer_view.cursor.line_index as usize;
            if was_inside_selection != inside_selection {
                was_inside_selection = inside_selection;
                if inside_selection {
                    handle_command!(
                        write,
                        SetForegroundColor(convert_color(editor.theme.background))
                    )?;
                    handle_command!(
                        write,
                        SetBackgroundColor(convert_color(editor.theme.foreground))
                    )?;
                } else {
                    handle_command!(
                        write,
                        SetForegroundColor(convert_color(editor.theme.foreground))
                    )?;
                    handle_command!(
                        write,
                        SetBackgroundColor(convert_color(editor.theme.background))
                    )?;
                }
            }

            match c {
                '\t' => {
                    for _ in 0..editor.config.tab_size {
                        handle_command!(write, Print(' '))?
                    }
                }
                _ => handle_command!(write, Print(c))?,
            }
        }

        handle_command!(
            write,
            SetForegroundColor(convert_color(editor.theme.foreground))
        )?;
        handle_command!(
            write,
            SetBackgroundColor(convert_color(editor.theme.background))
        )?;
        handle_command!(write, Clear(ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;
    }

    handle_command!(
        write,
        SetForegroundColor(convert_color(editor.theme.foreground))
    )?;
    handle_command!(
        write,
        SetBackgroundColor(convert_color(editor.theme.background))
    )?;
    for _ in buffer.lines.len()..buffer_view.size.1 as usize {
        handle_command!(write, Print('~'))?;
        handle_command!(write, Clear(ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;
    }

    handle_command!(write, ResetColor)?;
    write.flush()?;
    Ok(())
}
