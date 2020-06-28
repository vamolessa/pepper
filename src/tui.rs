use std::{io::Write, iter};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
    Result,
};

use crate::{
    buffer_position::BufferPosition,
    editor::Editor,
    event::{Event, Key},
    theme,
    viewport::Viewport,
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

fn update_viewports_size(editor: &mut Editor) {
    let size = terminal::size().unwrap_or((0, 0));
    editor.viewports.set_view_size((size.0 as _, size.1 as _));
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

    update_viewports_size(&mut editor);
    draw(&mut write, &editor)?;

    while editor.on_event(convert_event(event::read()?)) {
        draw(&mut write, &editor)?;
    }

    handle_command!(write, terminal::LeaveAlternateScreen)?;
    handle_command!(write, cursor::Show)?;
    terminal::disable_raw_mode().unwrap();

    Ok(())
}

fn draw<W>(write: &mut W, editor: &Editor) -> Result<()>
where
    W: Write,
{
    handle_command!(write, cursor::Hide)?;
    for viewport in editor.viewports.iter() {
        draw_viewport(write, editor, viewport)?;
    }
    Ok(())
}

fn draw_viewport<W>(write: &mut W, editor: &Editor, viewport: &Viewport) -> Result<()>
where
    W: Write,
{
    let buffer_view = match viewport.buffer_view_index() {
        Some(index) => &editor.buffer_views[index],
        None => return Ok(()),
    };
    let buffer = &editor.buffers[buffer_view.buffer_handle];

    handle_command!(
        write,
        cursor::MoveTo(viewport.position.0 as _, viewport.position.1 as _)
    )?;
    handle_command!(
        write,
        SetForegroundColor(convert_color(editor.theme.foreground))
    )?;
    handle_command!(
        write,
        SetBackgroundColor(convert_color(editor.theme.background))
    )?;

    for (y, line) in buffer
        .content
        .lines()
        .skip(viewport.scroll)
        .take(viewport.size.1)
        .enumerate()
    {
        let mut was_inside_selection = false;
        let y = y + viewport.scroll;
        for (x, c) in line
            .text
            .chars()
            .take(viewport.size.0 - 2)
            .chain(iter::once(' '))
            .enumerate()
        {
            let char_position = BufferPosition::line_col(y, x);
            let on_cursor = buffer_view
                .cursors
                .iter()
                .any(|c| char_position == c.position);
            let inside_selection = if on_cursor {
                false
            } else {
                buffer_view
                    .cursors
                    .iter()
                    .any(|c| c.range().contains(char_position))
            };

            if on_cursor {
                handle_command!(
                    write,
                    SetBackgroundColor(convert_color(editor.theme.cursor))
                )?;
            } else if was_inside_selection != inside_selection {
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

            if on_cursor {
                handle_command!(
                    write,
                    SetBackgroundColor(convert_color(editor.theme.background))
                )?;
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
        handle_command!(write, cursor::MoveToColumn(viewport.position.0 as _))?;
    }

    handle_command!(
        write,
        SetForegroundColor(convert_color(editor.theme.foreground))
    )?;
    handle_command!(
        write,
        SetBackgroundColor(convert_color(editor.theme.background))
    )?;
    for _ in buffer.content.line_count()..viewport.size.1 {
        handle_command!(write, Print('~'))?;
        handle_command!(write, Clear(ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;
        handle_command!(write, cursor::MoveToColumn(viewport.position.0 as _))?;
    }

    handle_command!(write, ResetColor)?;
    write.flush()?;
    Ok(())
}
