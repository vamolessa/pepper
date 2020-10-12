use std::{io, io::Write, iter, sync::mpsc, thread};

use crossterm::{
    cursor, event, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal,
    tty::IsTty,
};

use crate::{
    buffer::{Buffer, BufferContent},
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
    buffer: Option<&'a Buffer>,
    main_cursor: Cursor,
    cursors: &'a [Cursor],
}

impl<'a> ClientView<'a> {
    pub fn from(editor: &'a Editor, client: &'a Client) -> ClientView<'a> {
        let buffer_view = client
            .current_buffer_view_handle
            .and_then(|h| editor.buffer_views.get(h));
        let buffer = buffer_view
            .map(|v| v.buffer_handle)
            .and_then(|h| editor.buffers.get(h));

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
            buffer,
            main_cursor,
            cursors,
        }
    }
}

fn draw_text<W>(write: &mut W, editor: &Editor, client_view: &ClientView) -> UiResult<()>
where
    W: Write,
{
    static EMPTY_BUFFER: BufferContent = BufferContent::empty();
    static EMPTY_HIGHLIGHTED_BUFFER: HighlightedBuffer = HighlightedBuffer::new();

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
        Mode::Insert(_) => convert_color(theme.cursor_insert),
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

    let cursors = &client_view.cursors[..];
    let cursors_last_index = cursors.len().saturating_sub(1);

    let buffer_content;
    let highlighted_buffer;
    let search_ranges;
    match client_view.buffer {
        Some(buffer) => {
            buffer_content = buffer.content();
            highlighted_buffer = buffer.highlighted();
            search_ranges = buffer.search_ranges();
        }
        None => {
            buffer_content = &EMPTY_BUFFER;
            highlighted_buffer = &EMPTY_HIGHLIGHTED_BUFFER;
            search_ranges = &[];
        }
    }
    let search_ranges_last_index = search_ranges.len().saturating_sub(1);

    let mut current_cursor_index = 0;
    let (mut current_cursor_position, mut current_cursor_range) =
        match cursors.get(current_cursor_index) {
            Some(cursor) => (cursor.position, cursor.as_range()),
            None => Default::default(),
        };

    let mut current_search_range_index = 0;
    let mut current_search_range = match search_ranges.get(current_search_range_index) {
        Some(&range) => range,
        None => BufferRange::default(),
    };

    'lines_loop: for line in buffer_content.lines().skip(line_index) {
        let mut draw_state = DrawState::Token(TokenKind::Text);
        let mut column_byte_index = 0;
        let mut x = 0;

        handle_command!(write, SetForegroundColor(token_text_color))?;

        for (char_index, c) in line.as_str().char_indices().chain(iter::once((0, '\0'))) {
            if x >= width {
                handle_command!(write, cursor::MoveToNextLine(1))?;

                drawn_line_count += 1;
                x -= width;

                if drawn_line_count >= height {
                    break 'lines_loop;
                }
            }

            let char_position = BufferPosition::line_col(line_index, column_byte_index);

            let token_kind = if c.is_ascii_whitespace() {
                TokenKind::Whitespace
            } else {
                highlighted_buffer.find_token_kind_at(line_index, char_index)
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

            if current_cursor_range.to < char_position && current_cursor_index < cursors_last_index
            {
                current_cursor_index += 1;
                let cursor = cursors[current_cursor_index];
                current_cursor_position = cursor.position;
                current_cursor_range = cursor.as_range();
            }
            let inside_current_cursor_range = current_cursor_range.from <= char_position
                && char_position <= current_cursor_range.to;

            if current_search_range.to <= char_position
                && current_search_range_index < search_ranges_last_index
            {
                current_search_range_index += 1;
                current_search_range = search_ranges[current_search_range_index];
            }
            let inside_current_search_range = current_search_range.from <= char_position
                && char_position < current_search_range.to;

            if char_position == current_cursor_position {
                if draw_state != DrawState::Cursor {
                    draw_state = DrawState::Cursor;
                    handle_command!(write, SetBackgroundColor(cursor_color))?;
                    handle_command!(write, SetForegroundColor(text_color))?;
                }
            } else if inside_current_cursor_range {
                if draw_state != DrawState::Selection(token_kind) {
                    draw_state = DrawState::Selection(token_kind);
                    handle_command!(write, SetBackgroundColor(text_color))?;
                    handle_command!(write, SetForegroundColor(background_color))?;
                }
            } else if inside_current_search_range {
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

            column_byte_index += c.len_utf8();
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
        let (status_message_kind, status_message) = editor.status_message.message();
        let status_message = status_message.trim();

        if status_message.is_empty() {
            match editor.mode {
                Mode::Normal(_) => Some(0),
                Mode::Insert(_) => {
                    let text = "-- INSERT --";
                    handle_command!(write, Print(text))?;
                    Some(text.len())
                }
                Mode::Search(_)
                | Mode::ScriptPicker(_)
                | Mode::Goto(_)
                | Mode::Script(_)
                | Mode::ScriptReadLine(_) => {
                    let read_line = &editor.read_line;

                    handle_command!(write, Print(read_line.prompt()))?;
                    handle_command!(write, Print(read_line.input()))?;
                    handle_command!(write, SetBackgroundColor(cursor_color))?;
                    handle_command!(write, Print(' '))?;
                    handle_command!(write, SetBackgroundColor(background_color))?;
                    None
                }
            }
        } else {
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
        }
    } else {
        Some(0)
    };

    if let Some((buffer, x)) = client_view.buffer.zip(x) {
        let buffer_path = buffer.path().and_then(|p| p.to_str()).unwrap_or("");
        let needs_save_text = if buffer.needs_save() { "*" } else { "" };

        let line_number = client_view.main_cursor.position.line_index + 1;
        let column_number = client_view.main_cursor.position.column_byte_index + 1;

        let param_count = match &editor.mode {
            Mode::Normal(state) => state.count,
            _ => 0,
        };
        let param_count_digit_count = if param_count > 0 {
            find_digit_count(param_count)
        } else {
            0
        };

        let line_digit_count = find_digit_count(line_number);
        let column_digit_count = find_digit_count(column_number);
        let buffer_status_len = x
            + 1
            + param_count_digit_count
            + editor
                .buffered_keys
                .iter()
                .map(|k| k.display_len())
                .fold(0, std::ops::Add::add)
            + needs_save_text.len()
            + buffer_path.len()
            + 1
            + line_digit_count
            + 1
            + column_digit_count
            + 1;

        let skip = (client_view.client.viewport_size.0 as usize).saturating_sub(buffer_status_len);
        for _ in 0..skip {
            handle_command!(write, Print(' '))?;
        }

        if param_count > 0 {
            handle_command!(write, Print(param_count))?;
        }
        for key in editor.buffered_keys.iter() {
            handle_command!(write, Print(key))?;
        }
        handle_command!(write, Print(' '))?;
        handle_command!(write, Print(needs_save_text))?;
        handle_command!(write, Print(buffer_path))?;
        handle_command!(write, Print(':'))?;
        handle_command!(write, Print(line_number))?;
        handle_command!(write, Print(','))?;
        handle_command!(write, Print(column_number))?;
    }

    handle_command!(write, terminal::Clear(terminal::ClearType::UntilNewLine))?;
    Ok(())
}
