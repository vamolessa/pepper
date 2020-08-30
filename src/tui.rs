use std::{cmp::Ordering, io::Write, iter, sync::mpsc, thread};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal, ErrorKind, Result,
};

use crate::{
    application::UI,
    buffer::BufferContent,
    buffer_position::{BufferPosition, BufferRange},
    client::{Client, TargetClient},
    client_event::{Key, LocalEvent},
    cursor::CursorCollection,
    editor::{Editor, StatusMessageKind},
    mode::Mode,
    syntax::TokenKind,
    theme,
};

fn convert_event(event: event::Event) -> LocalEvent {
    match event {
        event::Event::Key(e) => match e.code {
            event::KeyCode::Backspace => LocalEvent::Key(Key::Backspace),
            event::KeyCode::Enter => LocalEvent::Key(Key::Enter),
            event::KeyCode::Left => LocalEvent::Key(Key::Left),
            event::KeyCode::Right => LocalEvent::Key(Key::Right),
            event::KeyCode::Up => LocalEvent::Key(Key::Up),
            event::KeyCode::Down => LocalEvent::Key(Key::Down),
            event::KeyCode::Home => LocalEvent::Key(Key::Home),
            event::KeyCode::End => LocalEvent::Key(Key::End),
            event::KeyCode::PageUp => LocalEvent::Key(Key::PageUp),
            event::KeyCode::PageDown => LocalEvent::Key(Key::PageDown),
            event::KeyCode::Tab => LocalEvent::Key(Key::Tab),
            event::KeyCode::Delete => LocalEvent::Key(Key::Delete),
            event::KeyCode::F(f) => LocalEvent::Key(Key::F(f)),
            event::KeyCode::Char('\0') => LocalEvent::None,
            event::KeyCode::Char(c) => match e.modifiers {
                event::KeyModifiers::CONTROL => LocalEvent::Key(Key::Ctrl(c)),
                event::KeyModifiers::ALT => LocalEvent::Key(Key::Alt(c)),
                _ => LocalEvent::Key(Key::Char(c)),
            },
            event::KeyCode::Esc => LocalEvent::Key(Key::Esc),
            _ => LocalEvent::None,
        },
        event::Event::Resize(w, h) => LocalEvent::Resize(w, h),
        _ => LocalEvent::None,
    }
}

const fn convert_color(color: theme::Color) -> Color {
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
    text_scroll: usize,
    select_scroll: usize,
}

impl<W> Tui<W>
where
    W: Write,
{
    pub fn new(write: W) -> Self {
        Self {
            write,
            text_scroll: 0,
            select_scroll: 0,
        }
    }
}

impl<W> UI for Tui<W>
where
    W: Write,
{
    type Error = ErrorKind;

    fn run_event_loop_in_background(
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> thread::JoinHandle<Result<()>> {
        thread::spawn(move || {
            while event_sender.send(convert_event(event::read()?)).is_ok() {}
            Ok(())
        })
    }

    fn init(&mut self) -> Result<()> {
        handle_command!(self.write, terminal::EnterAlternateScreen)?;
        handle_command!(self.write, cursor::Hide)?;
        self.write.flush()?;
        terminal::enable_raw_mode()?;
        Ok(())
    }

    fn draw_into(
        &mut self,
        editor: &Editor,
        client: &Client,
        target_client: TargetClient,
        buffer: &mut Vec<u8>,
    ) -> Result<()> {
        let has_focus = target_client == editor.focused_client;
        let client_view = get_client_view(editor, client);

        draw_text(
            buffer,
            editor,
            client,
            client_view,
            target_client,
        )?;
        draw_select(
            buffer,
            editor,
            client,
            client_view,
        )?;
        draw_statusbar(
            buffer,
            editor,
            client,
            client_view,
            has_focus,
        )?;

        handle_command!(self.write, ResetColor)?;
        self.write.flush()?;
        Ok(())
    }

    fn display(&mut self, buffer: &[u8]) -> Result<()> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        handle_command!(self.write, terminal::Clear(terminal::ClearType::All))?;
        handle_command!(self.write, terminal::LeaveAlternateScreen)?;
        handle_command!(self.write, ResetColor)?;
        handle_command!(self.write, cursor::Show)?;
        self.write.flush()?;
        terminal::disable_raw_mode()?;
        Ok(())
    }
}

static EMPTY_BUFFER: BufferContent = BufferContent::from_str("");
static EMPTY_CURSORS: CursorCollection = CursorCollection::new();

struct ClientView<'a> {
    buffer: &'a BufferContent,
    cursors: &'a CursorCollection,
    search_ranges: &'a [BufferRange],
}

fn get_client_view<'a>(editor: &'a Editor, client: &Client) -> ClientView<'a> {
    let buffer_view = client
        .current_buffer_view_handle
        .and_then(|h| editor.buffer_views.get(h));
    let buffer = buffer_view
        .map(|v| v.buffer_handle)
        .and_then(|h| editor.buffers.get(h));

    let content;
    let search_ranges;
    match buffer {
        Some(buffer) => {
            content = &buffer.content;
            search_ranges = buffer.search_ranges();
        }
        None => {
            content = &EMPTY_BUFFER;
            search_ranges = &[];
        }
    }

    let cursors = match buffer_view {
        Some(view) => &view.cursors,
        None => &EMPTY_CURSORS,
    };

    ClientView {
        buffer: content,
        cursors,
        search_ranges,
    }
}

fn draw_text<W>(
    write: &mut W,
    editor: &Editor,
    client: &Client,
    client_view: ClientView,
    target_client: TargetClient,
) -> Result<()>
where
    W: Write,
{
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum DrawState {
        Token(TokenKind),
        Selection(TokenKind),
        Highlight,
        Cursor,
    }

    let scroll = client.text_scroll;
    let width = client.width;
    let height = client.text_height;
    let theme = &editor.config.theme;

    handle_command!(write, cursor::Hide)?;

    let cursor_color = match editor.mode {
        Mode::Select => convert_color(theme.cursor_select),
        Mode::Insert => convert_color(theme.cursor_insert),
        _ => convert_color(theme.cursor_normal),
    };

    let background_color = convert_color(theme.background);
    let token_whitespace_color = convert_color(theme.token_whitespace);
    let token_text_color = convert_color(theme.token_text);
    let token_comment_color = convert_color(theme.token_comment);
    let token_keyword_color = convert_color(theme.token_keyword);
    let token_modifier_color = convert_color(theme.token_type);
    let token_symbol_color = convert_color(theme.token_symbol);
    let token_string_color = convert_color(theme.token_string);
    let token_literal_color = convert_color(theme.token_literal);
    let highlight_color = convert_color(theme.highlight);

    let mut text_color = token_text_color;

    handle_command!(write, cursor::MoveTo(0, 0))?;
    handle_command!(write, SetBackgroundColor(background_color))?;
    handle_command!(write, SetForegroundColor(text_color))?;

    let mut line_index = scroll;
    let mut drawn_line_count = 0;

    'lines_loop: for line in client_view.buffer.lines_from(line_index) {
        let mut draw_state = DrawState::Token(TokenKind::Text);
        let mut column_index = 0;
        let mut x = 0;

        handle_command!(write, SetForegroundColor(token_text_color))?;

        for (raw_char_index, c) in line.text(..).char_indices().chain(iter::once((0, '\0'))) {
            if x >= width {
                handle_command!(write, cursor::MoveToNextLine(1))?;

                drawn_line_count += 1;
                x -= width;

                if drawn_line_count >= height {
                    break 'lines_loop;
                }
            }

            let char_position = BufferPosition::line_col(line_index, column_index);

            let token_kind = if c.is_ascii_whitespace() {
                TokenKind::Whitespace
            } else {
                client
                    .highlighted_buffer
                    .find_token_kind_at(line_index, raw_char_index)
            };

            text_color = match token_kind {
                TokenKind::Whitespace => token_whitespace_color,
                TokenKind::Text => token_text_color,
                TokenKind::Comment => token_comment_color,
                TokenKind::Keyword => token_keyword_color,
                TokenKind::Type => token_modifier_color,
                TokenKind::Symbol => token_symbol_color,
                TokenKind::String => token_string_color,
                TokenKind::Literal => token_literal_color,
            };

            if client_view.cursors[..]
                .binary_search_by_key(&char_position, |c| c.position)
                .is_ok()
            {
                if draw_state != DrawState::Cursor {
                    draw_state = DrawState::Cursor;
                    handle_command!(write, SetBackgroundColor(cursor_color))?;
                    handle_command!(write, SetForegroundColor(text_color))?;
                }
            } else if client_view.cursors[..]
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
                if draw_state != DrawState::Selection(token_kind) {
                    draw_state = DrawState::Selection(token_kind);
                    handle_command!(write, SetBackgroundColor(text_color))?;
                    handle_command!(write, SetForegroundColor(background_color))?;
                }
            } else if client_view.search_ranges
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
                if draw_state != DrawState::Highlight {
                    draw_state = DrawState::Highlight;
                    handle_command!(write, SetBackgroundColor(highlight_color))?;
                    handle_command!(write, SetForegroundColor(background_color))?;
                }
            } else if draw_state != DrawState::Token(token_kind) {
                draw_state = DrawState::Token(token_kind);
                handle_command!(write, SetBackgroundColor(background_color))?;
                handle_command!(write, SetForegroundColor(text_color))?;
            }

            match c {
                '\0' => {
                    handle_command!(write, Print(' '))?;
                    x += 1;
                }
                ' ' => {
                    handle_command!(write, Print(editor.config.values.visual_space))?;
                    x += 1;
                }
                '\t' => {
                    handle_command!(write, Print(editor.config.values.visual_tab_first))?;
                    let tab_size = editor.config.values.tab_size.get() as u16;
                    let next_tab_stop = (tab_size - 1) - x % tab_size;
                    for _ in 0..next_tab_stop {
                        handle_command!(write, Print(editor.config.values.visual_tab_repeat))?;
                    }
                    x += tab_size;
                }
                _ => {
                    handle_command!(write, Print(c))?;
                    x += 1;
                }
            }

            column_index += 1;
        }

        if x < width {
            handle_command!(write, SetBackgroundColor(background_color))?;
            handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
        }

        handle_command!(write, cursor::MoveToNextLine(1))?;

        line_index += 1;
        drawn_line_count += 1;

        if drawn_line_count >= height {
            break;
        }
    }

    handle_command!(write, SetBackgroundColor(background_color))?;
    handle_command!(write, SetForegroundColor(token_whitespace_color))?;
    for _ in drawn_line_count..height {
        handle_command!(write, Print(editor.config.values.visual_empty))?;
        handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;
    }

    Ok(())
}

fn draw_select<W>(
    write: &mut W,
    editor: &Editor,
    client: &Client,
    scroll: usize,
    _width: u16,
    height: u16,
) -> Result<()>
where
    W: Write,
{
    let background_color = convert_color(editor.config.theme.token_whitespace);
    let foreground_color = convert_color(editor.config.theme.token_text);

    handle_command!(write, SetBackgroundColor(background_color))?;
    handle_command!(write, SetForegroundColor(foreground_color))?;

    for entry in editor.selects.entries_from(scroll).take(height as _) {
        handle_command!(write, Print(&entry.name[..]))?;
        handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;
    }

    Ok(())
}

fn draw_statusbar<W>(
    write: &mut W,
    editor: &Editor,
    client: &Client,
    has_focus: bool,
    width: u16,
    status_message_kind: StatusMessageKind,
    status_message: &str,
) -> Result<()>
where
    W: Write,
{
    fn draw_input<W>(
        write: &mut W,
        prefix: &str,
        input: &str,
        background_color: Color,
        cursor_color: Color,
    ) -> Result<usize>
    where
        W: Write,
    {
        handle_command!(write, Print(prefix))?;
        handle_command!(write, Print(input))?;
        handle_command!(write, SetBackgroundColor(cursor_color))?;
        handle_command!(write, Print(' '))?;
        handle_command!(write, SetBackgroundColor(background_color))?;
        Ok(prefix.len() + input.len() + 1)
    }

    fn find_digit_count(mut number: usize) -> usize {
        let mut count = 0;
        while number > 0 {
            number /= 10;
            count += 1;
        }
        count
    }

    let background_color = convert_color(editor.config.theme.token_text);
    let foreground_color = convert_color(editor.config.theme.background);
    let cursor_color = convert_color(editor.config.theme.cursor_normal);

    if has_focus {
        handle_command!(write, SetBackgroundColor(background_color))?;
        handle_command!(write, SetForegroundColor(foreground_color))?;
    } else {
        handle_command!(write, SetBackgroundColor(foreground_color))?;
        handle_command!(write, SetForegroundColor(background_color))?;
    }

    let x = if !status_message.is_empty() {
        let prefix = match status_message_kind {
            StatusMessageKind::Info => "",
            StatusMessageKind::Error => "error:",
        };

        let line_count = status_message.lines().count();
        if line_count > 1 {
            if prefix.is_empty() {
                handle_command!(write, cursor::MoveUp((line_count - 1) as _))?;
                handle_command!(write, terminal::Clear(terminal::ClearType::FromCursorDown))?;
            } else {
                handle_command!(write, cursor::MoveUp(line_count as _))?;
                handle_command!(write, Print(prefix))?;
                handle_command!(write, terminal::Clear(terminal::ClearType::FromCursorDown))?;
                handle_command!(write, cursor::MoveToNextLine(1))?;
            }

            for (i, line) in status_message.lines().enumerate() {
                handle_command!(write, Print(line))?;
                if i < line_count - 1 {
                    handle_command!(write, cursor::MoveToNextLine(1))?;
                }
            }
        } else {
            handle_command!(write, terminal::Clear(terminal::ClearType::CurrentLine))?;
            handle_command!(write, Print(prefix))?;
            handle_command!(write, Print(status_message))?;
        }

        None
    } else if has_focus {
        match editor.mode {
            Mode::Select => {
                let text = "-- SELECT --";
                handle_command!(write, Print(text))?;
                Some(text.len())
            }
            Mode::Insert => {
                let text = "-- INSERT --";
                handle_command!(write, Print(text))?;
                Some(text.len())
            }
            Mode::Search(_) => Some(draw_input(
                write,
                "/",
                &editor.input[..],
                background_color,
                cursor_color,
            )?),
            Mode::Script(_) => Some(draw_input(
                write,
                ":",
                &editor.input[..],
                background_color,
                cursor_color,
            )?),
            _ => Some(0),
        }
    } else {
        Some(0)
    };

    if let Some(x) = x {
        if let Some(buffer_path) = client.path.as_os_str().to_str().filter(|s| !s.is_empty()) {
            let line_number = client.main_cursor.position.line_index + 1;
            let column_number = client.main_cursor.position.column_index + 1;
            let line_digit_count = find_digit_count(line_number);
            let column_digit_count = find_digit_count(column_number);
            let skip = (width as usize).saturating_sub(
                x + buffer_path.len() + 1 + line_digit_count + 1 + column_digit_count + 1,
            );
            for _ in 0..skip {
                handle_command!(write, Print(' '))?;
            }

            handle_command!(write, Print(buffer_path))?;
            handle_command!(write, Print(':'))?;
            handle_command!(write, Print(line_number))?;
            handle_command!(write, Print(','))?;
            handle_command!(write, Print(column_number))?;
        }
    }

    handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
    Ok(())
}
