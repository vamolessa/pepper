use std::{cmp::Ordering, io::Write, iter};

use futures::{future::FutureExt, StreamExt};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal, Result,
};

use crate::{
    buffer_position::BufferPosition,
    editor::{Editor, EditorPollResult},
    event::{Event, Key},
    mode::Mode,
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

pub fn show<W>(mut write: W, mut editor: Editor) -> Result<()>
where
    W: Write,
{
    handle_command!(write, terminal::EnterAlternateScreen)?;
    write.flush()?;
    handle_command!(write, cursor::Hide)?;
    write.flush()?;
    terminal::enable_raw_mode()?;

    let available_size = terminal::size()?;
    editor.on_event(Event::Resize(available_size.0 as _, available_size.1 as _));

    draw(&mut write, &editor, None)?;
    smol::run(main_loop(&mut write, &mut editor))?;

    handle_command!(write, terminal::LeaveAlternateScreen)?;
    handle_command!(write, cursor::Show)?;
    terminal::disable_raw_mode()?;

    Ok(())
}

async fn main_loop<W>(write: &mut W, editor: &mut Editor) -> Result<()>
where
    W: Write,
{
    let mut event_stream = event::EventStream::new();
    loop {
        let event = event_stream.next().fuse().await;
        let mut error = None;
        if let Some(event) = event {
            match editor.on_event(convert_event(event?)) {
                EditorPollResult::Pending => (),
                EditorPollResult::Quit => break,
                EditorPollResult::Error(e) => error = Some(e),
            }
        }
        draw(write, &editor, error)?;
    }
    Ok(())
}

fn draw<W>(write: &mut W, editor: &Editor, error: Option<String>) -> Result<()>
where
    W: Write,
{
    //handle_command!(write, SetBackgroundColor(Color::Red))?;
    //handle_command!(write, terminal::Clear(terminal::ClearType::All))?;

    handle_command!(write, cursor::Hide)?;
    for viewport in editor.viewports.iter() {
        draw_viewport(write, editor, viewport)?;
    }

    handle_command!(write, cursor::MoveToNextLine(1))?;
    draw_statusbar(write, editor, error)?;

    write.flush()?;
    Ok(())
}

fn draw_viewport<W>(write: &mut W, editor: &Editor, viewport: &Viewport) -> Result<()>
where
    W: Write,
{
    enum DrawState {
        Normal,
        Selection,
        Highlight,
        Cursor,
    }

    let cursor_color = match editor.mode() {
        Mode::Select => convert_color(editor.theme.cursor_select),
        Mode::Insert => convert_color(editor.theme.cursor_insert),
        _ => convert_color(editor.theme.cursor_normal),
    };

    handle_command!(write, cursor::MoveTo(viewport.x as _, 0))?;
    handle_command!(
        write,
        SetBackgroundColor(convert_color(editor.theme.background))
    )?;
    handle_command!(
        write,
        SetForegroundColor(convert_color(editor.theme.text_normal))
    )?;

    let (buffer_view, buffer) = match viewport
        .current_buffer_view_handle()
        .map(|h| editor.buffer_views.get(h))
        .and_then(|bv| editor.buffers.get(bv.buffer_handle).map(|b| (bv, b)))
    {
        Some((buffer_view, buffer)) => (buffer_view, buffer),
        None => {
            for _ in 0..viewport.height {
                handle_command!(write, Print('~'))?;
                for _ in 0..viewport.width - 1 {
                    handle_command!(write, Print(' '))?;
                }
                handle_command!(write, cursor::MoveToNextLine(1))?;
                handle_command!(write, cursor::MoveToColumn((viewport.x + 1) as _))?;
            }
            return Ok(());
        }
    };

    let mut line_index = viewport.scroll;
    let mut drawn_line_count = 0;

    'lines_loop: for line in buffer.content.lines_from(viewport.scroll) {
        let mut draw_state = DrawState::Normal;
        let mut column_index = 0;
        let mut x = 0;

        for c in line.text.chars().chain(iter::once(' ')) {
            if x >= viewport.width {
                handle_command!(write, cursor::MoveToNextLine(1))?;
                handle_command!(write, cursor::MoveToColumn((viewport.x + 1) as _))?;

                draw_state = DrawState::Normal;
                drawn_line_count += 1;
                x = 0;

                if drawn_line_count == viewport.height {
                    break 'lines_loop;
                }
            }

            let char_position = BufferPosition::line_col(line_index, column_index);
            if buffer_view.cursors[..]
                .binary_search_by_key(&char_position, |c| c.position)
                .is_ok()
            {
                if !matches!(draw_state, DrawState::Cursor) {
                    draw_state = DrawState::Cursor;
                    handle_command!(write, SetBackgroundColor(cursor_color))?;
                    handle_command!(
                        write,
                        SetForegroundColor(convert_color(editor.theme.text_normal))
                    )?;
                }
            } else if buffer_view.cursors[..]
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
                    handle_command!(
                        write,
                        SetBackgroundColor(convert_color(editor.theme.text_normal))
                    )?;
                    handle_command!(
                        write,
                        SetForegroundColor(convert_color(editor.theme.background))
                    )?;
                }
            } else if buffer
                .search_ranges()
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
                    handle_command!(
                        write,
                        SetBackgroundColor(convert_color(editor.theme.highlight))
                    )?;
                    handle_command!(
                        write,
                        SetForegroundColor(convert_color(editor.theme.text_normal))
                    )?;
                }
            } else if !matches!(draw_state, DrawState::Normal) {
                draw_state = DrawState::Normal;
                handle_command!(
                    write,
                    SetBackgroundColor(convert_color(editor.theme.background))
                )?;
                handle_command!(
                    write,
                    SetForegroundColor(convert_color(editor.theme.text_normal))
                )?;
            }

            match c {
                '\t' => {
                    for _ in 0..editor.config.tab_size {
                        handle_command!(write, Print(' '))?
                    }
                    column_index += editor.config.tab_size;
                    x += editor.config.tab_size;
                }
                _ => {
                    handle_command!(write, Print(c))?;
                    column_index += 1;
                    x += 1;
                }
            }
        }

        handle_command!(
            write,
            SetBackgroundColor(convert_color(editor.theme.background))
        )?;
        handle_command!(
            write,
            SetForegroundColor(convert_color(editor.theme.text_normal))
        )?;
        for _ in x..viewport.width {
            handle_command!(write, Print(' '))?;
        }
        handle_command!(write, cursor::MoveToNextLine(1))?;
        handle_command!(write, cursor::MoveToColumn((viewport.x + 1) as _))?;

        line_index += 1;
        drawn_line_count += 1;

        if drawn_line_count == viewport.height {
            break;
        }
    }

    handle_command!(
        write,
        SetBackgroundColor(convert_color(editor.theme.background))
    )?;
    handle_command!(
        write,
        SetForegroundColor(convert_color(editor.theme.text_normal))
    )?;
    for _ in drawn_line_count..viewport.height {
        handle_command!(write, Print('~'))?;
        for _ in 0..(viewport.width - 1) {
            handle_command!(write, Print(' '))?;
        }
        handle_command!(write, cursor::MoveToNextLine(1))?;
        handle_command!(write, cursor::MoveToColumn((viewport.x + 1) as _))?;
    }

    if viewport.is_current {
        handle_command!(
            write,
            SetBackgroundColor(convert_color(editor.theme.text_normal))
        )?;
        handle_command!(
            write,
            SetForegroundColor(convert_color(editor.theme.background))
        )?;
    } else {
        handle_command!(
            write,
            SetBackgroundColor(convert_color(editor.theme.background))
        )?;
        handle_command!(
            write,
            SetForegroundColor(convert_color(editor.theme.text_normal))
        )?;
    }
    let buffer_name = "the buffer name";
    handle_command!(write, Print(buffer_name))?;
    for _ in buffer_name.len()..(viewport.width - 1) {
        handle_command!(write, Print(' '))?;
    }

    handle_command!(write, ResetColor)?;
    Ok(())
}

fn draw_statusbar<W>(write: &mut W, editor: &Editor, error: Option<String>) -> Result<()>
where
    W: Write,
{
    fn draw_input<W>(write: &mut W, prefix: &str, editor: &Editor) -> Result<()>
    where
        W: Write,
    {
        handle_command!(write, Print(prefix))?;
        handle_command!(write, Print(editor.input()))?;
        handle_command!(
            write,
            SetBackgroundColor(convert_color(editor.theme.cursor_normal))
        )?;
        handle_command!(write, Print(' '))?;
        handle_command!(
            write,
            SetBackgroundColor(convert_color(editor.theme.background))
        )?;
        Ok(())
    }

    handle_command!(
        write,
        SetBackgroundColor(convert_color(editor.theme.background))
    )?;
    handle_command!(
        write,
        SetForegroundColor(convert_color(editor.theme.text_normal))
    )?;

    if let Some(error) = error {
        handle_command!(write, Print("error: "))?;
        handle_command!(write, Print(error))?;
    } else {
        match editor.mode() {
            Mode::Select => handle_command!(write, Print("-- SELECT --"))?,
            Mode::Insert => handle_command!(write, Print("-- INSERT --"))?,
            Mode::Search(_) => draw_input(write, "search: ", editor)?,
            Mode::Command(_) => draw_input(write, "command: ", editor)?,
            _ => (),
        };
    }

    handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
    Ok(())
}
