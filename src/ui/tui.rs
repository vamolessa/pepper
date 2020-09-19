use std::{cmp::Ordering, io, io::Write, iter, path::Path, sync::mpsc, thread};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal,
    tty::IsTty,
};

use crate::{
    buffer::BufferContent,
    buffer_position::{BufferPosition, BufferRange},
    client::{Client, TargetClient},
    client_event::{Key, LocalEvent},
    cursor::Cursor,
    editor::{Editor, StatusMessageKind},
    mode::Mode,
    syntax::{HighlightedBuffer, TokenKind},
    theme,
};

use super::{read_keys_from_stdin, Ui, UiResult};

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
}

impl<W> Tui<W>
where
    W: Write,
{
    pub fn new(write: W) -> Self {
        Self { write }
    }
}

impl<W> Ui for Tui<W>
where
    W: Write,
{
    fn run_event_loop_in_background(
        &mut self,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> thread::JoinHandle<()> {
        if io::stdin().is_tty() {
            let size = terminal::size().unwrap_or((0, 0));
            let _ = event_sender.send(LocalEvent::Resize(size.0, size.1));

            thread::spawn(move || {
                loop {
                    let event = match event::read() {
                        Ok(event) => event,
                        Err(_) => break,
                    };

                    if event_sender.send(convert_event(event)).is_err() {
                        break;
                    }
                }

                let _ = event_sender.send(LocalEvent::EndOfInput);
            })
        } else {
            thread::spawn(move || read_keys_from_stdin(event_sender))
        }
    }

    fn init(&mut self) -> UiResult<()> {
        handle_command!(self.write, terminal::EnterAlternateScreen)?;
        handle_command!(self.write, cursor::Hide)?;
        self.write.flush()?;
        terminal::enable_raw_mode()?;
        Ok(())
    }

    fn display(&mut self, buffer: &[u8]) -> UiResult<()> {
        self.write.write_all(buffer)?;
        handle_command!(self.write, ResetColor)?;
        self.write.flush()?;
        Ok(())
    }

    fn shutdown(&mut self) -> UiResult<()> {
        handle_command!(self.write, terminal::Clear(terminal::ClearType::All))?;
        handle_command!(self.write, terminal::LeaveAlternateScreen)?;
        handle_command!(self.write, ResetColor)?;
        handle_command!(self.write, cursor::Show)?;
        self.write.flush()?;
        terminal::disable_raw_mode()?;
        Ok(())
    }
}

pub fn render(
    editor: &Editor,
    client: &Client,
    target_client: TargetClient,
    buffer: &mut Vec<u8>,
) -> UiResult<()> {
    let has_focus = target_client == editor.focused_client;
    let client_view = ClientView::from(editor, client);

    draw_text(buffer, editor, &client_view)?;
    draw_picker(buffer, editor)?;
    draw_statusbar(buffer, editor, &client_view, has_focus)?;
    Ok(())
}

struct ClientView<'a> {
    client: &'a Client,
    buffer_path: &'a Path,
    buffer_content: &'a BufferContent,
    highlighted_buffer: &'a HighlightedBuffer,
    main_cursor: Cursor,
    cursors: &'a [Cursor],
    search_ranges: &'a [BufferRange],
}

impl<'a> ClientView<'a> {
    pub fn from(editor: &'a Editor, client: &'a Client) -> ClientView<'a> {
        static EMPTY_BUFFER: BufferContent = BufferContent::empty();
        static EMPTY_HIGHLIGHTED_BUFFER: HighlightedBuffer = HighlightedBuffer::new();

        let buffer_view = client
            .current_buffer_view_handle
            .and_then(|h| editor.buffer_views.get(h));
        let maybe_buffer = buffer_view
            .map(|v| v.buffer_handle)
            .and_then(|h| editor.buffers.get(h));

        let buffer_path;
        let buffer_content;
        let highlighted_buffer;
        let search_ranges;
        match maybe_buffer {
            Some(buffer) => {
                buffer_path = buffer.path();
                buffer_content = &buffer.content;
                highlighted_buffer = &buffer.highlighted;
                search_ranges = buffer.search_ranges();
            }
            None => {
                buffer_path = Path::new("");
                buffer_content = &EMPTY_BUFFER;
                highlighted_buffer = &EMPTY_HIGHLIGHTED_BUFFER;
                search_ranges = &[];
            }
        }

        let main_cursor;
        let cursors;
        match buffer_view {
            Some(view) => {
                main_cursor = view.cursors.main_cursor().clone();
                cursors = &view.cursors[..];
            }
            None => {
                main_cursor = Cursor::default();
                cursors = &[];
            }
        };

        ClientView {
            client,
            buffer_path,
            buffer_content,
            highlighted_buffer,
            main_cursor,
            cursors,
            search_ranges,
        }
    }
}

fn draw_text<W>(write: &mut W, editor: &Editor, client_view: &ClientView) -> UiResult<()>
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

    let scroll = client_view.client.scroll;
    let width = client_view.client.viewport_size.0;
    let height = client_view.client.height;
    let theme = &editor.config.theme;

    handle_command!(write, cursor::Hide)?;

    let cursor_color = match editor.mode {
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

    'lines_loop: for line in client_view.buffer_content.lines().skip(line_index) {
        let mut draw_state = DrawState::Token(TokenKind::Text);
        let mut column_index = 0;
        let mut x = 0;

        handle_command!(write, SetForegroundColor(token_text_color))?;

        for (raw_char_index, c) in line.as_str().char_indices().chain(iter::once((0, '\0'))) {
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
                client_view
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
            } else if client_view
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

fn draw_picker<W>(write: &mut W, editor: &Editor) -> UiResult<()>
where
    W: Write,
{
    let cursor = editor.picker.cursor();
    let scroll = editor.picker.scroll();
    let height = editor
        .picker
        .height(editor.config.values.picker_max_height.get());

    let background_color = convert_color(editor.config.theme.token_whitespace);
    let foreground_color = convert_color(editor.config.theme.token_text);

    handle_command!(write, SetBackgroundColor(background_color))?;
    handle_command!(write, SetForegroundColor(foreground_color))?;

    for (i, entry) in editor
        .picker
        .entries(&editor.word_database)
        .enumerate()
        .skip(scroll)
        .take(height)
    {
        if i == cursor {
            handle_command!(write, SetForegroundColor(background_color))?;
            handle_command!(write, SetBackgroundColor(foreground_color))?;
        } else if i == cursor + 1 {
            handle_command!(write, SetBackgroundColor(background_color))?;
            handle_command!(write, SetForegroundColor(foreground_color))?;
        }

        handle_command!(write, Print(&entry.name))?;
        handle_command!(write, Print(" - "))?;
        handle_command!(write, Print(entry.score))?;
        handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
        handle_command!(write, cursor::MoveToNextLine(1))?;
    }

    Ok(())
}

fn draw_statusbar<W>(
    write: &mut W,
    editor: &Editor,
    client_view: &ClientView,
    has_focus: bool,
) -> UiResult<()>
where
    W: Write,
{
    fn draw_input<W>(
        write: &mut W,
        prefix: &str,
        input: &str,
        background_color: Color,
        cursor_color: Color,
    ) -> UiResult<usize>
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

    let x = if has_focus {
        if editor.status_message.is_empty() {
            match editor.mode {
                Mode::Insert => {
                    let text = "-- INSERT --";
                    handle_command!(write, Print(text))?;
                    Some(text.len())
                }
                Mode::Search => {
                    draw_input(write, "/", &editor.input, background_color, cursor_color)?;
                    None
                }
                Mode::Script => {
                    draw_input(write, ":", &editor.input, background_color, cursor_color)?;
                    None
                }
                _ => Some(0),
            }
        } else {
            let prefix = match editor.status_message_kind {
                StatusMessageKind::Info => "",
                StatusMessageKind::Error => "error:",
            };

            let line_count = editor.status_message.lines().count();
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

                for (i, line) in editor.status_message.lines().enumerate() {
                    handle_command!(write, Print(line))?;
                    if i < line_count - 1 {
                        handle_command!(write, cursor::MoveToNextLine(1))?;
                    }
                }
            } else {
                handle_command!(write, terminal::Clear(terminal::ClearType::CurrentLine))?;
                handle_command!(write, Print(prefix))?;
                handle_command!(write, Print(&editor.status_message))?;
            }

            None
        }
    } else {
        Some(0)
    };

    if let Some((buffer_path, x)) = client_view
        .buffer_path
        .as_os_str()
        .to_str()
        .filter(|s| !s.is_empty())
        .zip(x)
    {
        let line_number = client_view.main_cursor.position.line_index + 1;
        let column_number = client_view.main_cursor.position.column_index + 1;
        let line_digit_count = find_digit_count(line_number);
        let column_digit_count = find_digit_count(column_number);
        let skip = (client_view.client.viewport_size.0 as usize).saturating_sub(
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

    handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
    Ok(())
}
