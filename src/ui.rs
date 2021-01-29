use std::{
    error::Error,
    io::{self, Write},
    iter,
};

use crossterm::{
    cursor, handle_command,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal, Command,
};

use crate::{
    buffer::{Buffer, BufferContent, BufferHandle},
    buffer_position::{BufferPosition, BufferRange},
    client::Client,
    cursor::Cursor,
    editor::{Editor, StatusMessageKind},
    mode::ModeKind,
    syntax::{HighlightedBuffer, TokenKind},
    theme,
};

/*
pub fn read_keys_from_stdin(event_sender: mpsc::Sender<LocalEvent>) {
    use io::BufRead;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut line = String::new();

    'main_loop: loop {
        match stdin.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => (),
        }

        for key in Key::parse_all(&line) {
            match key {
                Ok(key) => {
                    if event_sender.send(LocalEvent::Key(key)).is_err() {
                        break 'main_loop;
                    }
                }
                Err(_) => break,
            }
        }

        line.clear();
    }

    let _ = event_sender.send(LocalEvent::EndOfInput);
}
*/

pub const ENTER_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1B[?1049h";
pub const EXIT_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1B[?1049l";
pub const HIDE_CURSOR_CODE: &[u8] = b"\x1B[?25l";
pub const SHOW_CURSOR_CODE: &[u8] = b"\x1B[?25h";
pub const RESET_STYLE_CODE: &[u8] = b"\x1B[?0m";

fn set_title(buf: &mut Vec<u8>, title: &[u8]) {
    buf.extend_from_slice(b"\x1B]0;");
    buf.extend_from_slice(title);
    buf.extend_from_slice(b"{}\x07");
}

fn clear_all(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[2J");
}

fn clear_until_new_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[K");
}

fn move_cursor_to(buf: &mut Vec<u8>, x: usize, y: usize) {
    let _ = write!(buf, "\x1B[{};{}H", x, y);
}

fn move_cursor_to_next_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[1D");
}

fn move_cursor_up(buf: &mut Vec<u8>, count: usize) {
    let _ = write!(buf, "\x1B[{}A", count);
}

fn set_background_color(buf: &mut Vec<u8>, color: theme::Color) {
    let _ = write!(buf, "\x1B[48;2;{};{};{}", color.0, color.1, color.2);
}

fn set_foreground_color(buf: &mut Vec<u8>, color: theme::Color) {
    let _ = write!(buf, "\x1B[38;2;{};{};{}", color.0, color.1, color.2);
}

fn set_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[4m");
}

fn set_no_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[4m");
}

#[inline]
fn write_command<W, C>(writer: &mut W, command: C)
where
    W: io::Write,
    C: Command,
{
    let _ = handle_command!(writer, command);
}

const fn convert_color(color: theme::Color) -> Color {
    Color::Rgb {
        r: color.0,
        g: color.1,
        b: color.2,
    }
}

pub fn render(
    editor: &Editor,
    client: &Client,
    has_focus: bool,
    buffer: &mut Vec<u8>,
    status_bar_buf: &mut String,
) {
    let client_view = ClientView::from(editor, client);

    draw_buffer(buffer, editor, &client_view, has_focus);
    if has_focus {
        draw_picker(buffer, editor, &client_view);
    }
    draw_statusbar(buffer, editor, &client_view, has_focus, status_bar_buf);
}

struct ClientView<'a> {
    client: &'a Client,
    buffer_handle: Option<BufferHandle>,
    buffer: Option<&'a Buffer>,
    main_cursor_position: BufferPosition,
    cursors: &'a [Cursor],
}

impl<'a> ClientView<'a> {
    pub fn from(editor: &'a Editor, client: &'a Client) -> ClientView<'a> {
        let buffer_view = client
            .buffer_view_handle()
            .and_then(|h| editor.buffer_views.get(h));
        let buffer_handle = buffer_view.map(|v| v.buffer_handle);
        let buffer = buffer_handle.and_then(|h| editor.buffers.get(h));

        let main_cursor_position;
        let cursors;
        match buffer_view {
            Some(view) => {
                main_cursor_position = view.cursors.main_cursor().position;
                cursors = &view.cursors[..];
            }
            None => {
                main_cursor_position = BufferPosition::default();
                cursors = &[];
            }
        };

        ClientView {
            client,
            buffer_handle,
            buffer,
            main_cursor_position,
            cursors,
        }
    }
}

fn draw_buffer<W>(write: &mut W, editor: &Editor, client_view: &ClientView, has_focus: bool)
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

    let cursor_color = if has_focus && editor.mode.kind() == ModeKind::Insert {
        convert_color(theme.highlight)
    } else {
        convert_color(theme.cursor)
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

    write_command(write, cursor::MoveTo(0, 0));
    write_command(write, SetBackgroundColor(background_color));
    write_command(write, SetForegroundColor(text_color));

    let mut line_index = scroll;
    let mut drawn_line_count = 0;

    let cursors = &client_view.cursors[..];
    let cursors_end_index = cursors.len().saturating_sub(1);

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
            buffer_content = BufferContent::empty();
            highlighted_buffer = HighlightedBuffer::empty();
            search_ranges = &[];
        }
    }
    let search_ranges_end_index = search_ranges.len().saturating_sub(1);

    let diagnostics = match client_view.buffer_handle {
        Some(handle) => {
            let mut diagnostics: &[_] = &[];
            for client in editor.lsp.clients() {
                diagnostics = client.diagnostics().buffer_diagnostics(handle);
                if !diagnostics.is_empty() {
                    break;
                }
            }
            diagnostics
        }
        None => &[],
    };
    let diagnostics_end_index = diagnostics.len().saturating_sub(1);

    let mut current_cursor_index = 0;
    let mut current_cursor_position = BufferPosition::default();
    let mut current_cursor_range = BufferRange::default();
    if let Some(cursor) = cursors.get(current_cursor_index) {
        current_cursor_position = cursor.position;
        current_cursor_range = cursor.as_range();
    }

    let mut current_search_range_index = 0;
    let mut current_search_range = BufferRange::default();
    if let Some(&range) = search_ranges.get(current_search_range_index) {
        current_search_range = range;
    }

    let mut current_diagnostic_index = 0;
    let mut current_diagnostic_range = BufferRange::default();
    if let Some(diagnostic) = diagnostics.get(current_diagnostic_index) {
        current_diagnostic_range = diagnostic.utf16_range;
    }

    'lines_loop: for line in buffer_content.lines().skip(line_index) {
        let mut draw_state = DrawState::Token(TokenKind::Text);
        let mut was_inside_diagnostic_range = false;
        let mut column_byte_index = 0;
        let mut x = 0;

        write_command(write, SetForegroundColor(token_text_color));

        for (char_index, c) in line.as_str().char_indices().chain(iter::once((0, '\0'))) {
            if x >= width {
                write_command(write, cursor::MoveToNextLine(1));

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

            if current_cursor_range.to < char_position && current_cursor_index < cursors_end_index {
                current_cursor_index += 1;
                let cursor = cursors[current_cursor_index];
                current_cursor_position = cursor.position;
                current_cursor_range = cursor.as_range();
            }
            let inside_cursor_range = current_cursor_range.from <= char_position
                && char_position < current_cursor_range.to;

            if current_search_range.to <= char_position
                && current_search_range_index < search_ranges_end_index
            {
                current_search_range_index += 1;
                current_search_range = search_ranges[current_search_range_index];
            }
            let inside_search_range = current_search_range.from <= char_position
                && char_position < current_search_range.to;

            if current_diagnostic_range.to < char_position
                && current_diagnostic_index < diagnostics_end_index
            {
                current_diagnostic_index += 1;
                current_diagnostic_range = diagnostics[current_diagnostic_index].utf16_range;
            }
            let inside_diagnostic_range = current_diagnostic_range.from <= char_position
                && char_position < current_diagnostic_range.to;

            if inside_diagnostic_range != was_inside_diagnostic_range {
                was_inside_diagnostic_range = inside_diagnostic_range;
                if inside_diagnostic_range {
                    write_command(write, SetAttribute(Attribute::Underlined));
                } else {
                    write_command(write, SetAttribute(Attribute::NoUnderline));
                }
            }

            if char_position == current_cursor_position {
                if draw_state != DrawState::Cursor {
                    draw_state = DrawState::Cursor;
                    write_command(write, SetBackgroundColor(cursor_color));
                    write_command(write, SetForegroundColor(text_color));
                }
            } else if inside_cursor_range {
                if draw_state != DrawState::Selection(token_kind) {
                    draw_state = DrawState::Selection(token_kind);
                    write_command(write, SetBackgroundColor(text_color));
                    write_command(write, SetForegroundColor(background_color));
                }
            } else if inside_search_range {
                if draw_state != DrawState::Highlight {
                    draw_state = DrawState::Highlight;
                    write_command(write, SetBackgroundColor(highlight_color));
                    write_command(write, SetForegroundColor(background_color));
                }
            } else if draw_state != DrawState::Token(token_kind) {
                draw_state = DrawState::Token(token_kind);
                write_command(write, SetBackgroundColor(background_color));
                write_command(write, SetForegroundColor(text_color));
            }

            match c {
                '\0' => {
                    write_command(write, Print(' '));
                    x += 1;
                }
                ' ' => {
                    write_command(write, Print(editor.config.values.visual_space));
                    x += 1;
                }
                '\t' => {
                    write_command(write, Print(editor.config.values.visual_tab_first));
                    let tab_size = editor.config.values.tab_size.get() as u16;
                    let next_tab_stop = (tab_size - 1) - x % tab_size;
                    for _ in 0..next_tab_stop {
                        write_command(write, Print(editor.config.values.visual_tab_repeat));
                    }
                    x += next_tab_stop + 1;
                }
                _ => {
                    write_command(write, Print(c));
                    x += 1;
                }
            }

            column_byte_index += c.len_utf8();
        }

        if x < width {
            write_command(write, SetBackgroundColor(background_color));
            write_command(write, terminal::Clear(terminal::ClearType::UntilNewLine));
        }

        write_command(write, cursor::MoveToNextLine(1));

        line_index += 1;
        drawn_line_count += 1;

        if drawn_line_count >= height {
            break;
        }
    }

    write_command(write, SetBackgroundColor(background_color));
    write_command(write, SetForegroundColor(token_whitespace_color));
    for _ in drawn_line_count..height {
        write_command(write, Print(editor.config.values.visual_empty));
        write_command(write, terminal::Clear(terminal::ClearType::UntilNewLine));
        write_command(write, cursor::MoveToNextLine(1));
    }
}

fn draw_picker<W>(write: &mut W, editor: &Editor, client_view: &ClientView)
where
    W: Write,
{
    let cursor = editor.picker.cursor();
    let scroll = editor.picker.scroll();

    let half_width = client_view.client.viewport_size.0 / 2;
    let half_width = half_width.saturating_sub(1) as usize;

    let height = editor
        .picker
        .height(editor.config.values.picker_max_height.get() as _);

    let background_color = convert_color(editor.config.theme.token_text);
    let foreground_color = convert_color(editor.config.theme.token_whitespace);

    write_command(write, SetBackgroundColor(background_color));
    write_command(write, SetForegroundColor(foreground_color));

    for (i, entry) in editor
        .picker
        .entries(&editor.word_database)
        .enumerate()
        .skip(scroll)
        .take(height)
    {
        if i == cursor {
            write_command(write, SetForegroundColor(background_color));
            write_command(write, SetBackgroundColor(foreground_color));
        } else if i == cursor + 1 {
            write_command(write, SetBackgroundColor(background_color));
            write_command(write, SetForegroundColor(foreground_color));
        }

        let mut x = 0;

        macro_rules! print_char {
            ($c:expr) => {
                x += 1;
                match $c {
                    '\t' => write_command(write, Print(' ')),
                    c => write_command(write, Print(c)),
                }
            };
        }

        let name_char_count = entry.name.chars().count();
        if name_char_count < half_width {
            for c in entry.name.chars() {
                print_char!(c);
            }
        } else {
            write_command(write, Print("..."));
            x += 3;
            let name_char_count = name_char_count + 3;
            for c in entry
                .name
                .chars()
                .skip(name_char_count.saturating_sub(half_width))
            {
                print_char!(c);
            }
        }
        for _ in x..half_width {
            write_command(write, Print(' '));
        }
        write_command(write, Print('|'));
        x = 0;
        for c in entry.description.chars() {
            if x + 3 > half_width {
                write_command(write, Print("..."));
                break;
            }
            print_char!(c);
        }

        write_command(write, terminal::Clear(terminal::ClearType::UntilNewLine));
        write_command(write, cursor::MoveToNextLine(1));
    }
}

fn draw_statusbar<W>(
    write: &mut W,
    editor: &Editor,
    client_view: &ClientView,
    has_focus: bool,
    buf: &mut String,
) where
    W: Write,
{
    let background_color = convert_color(editor.config.theme.token_text);
    let foreground_color = convert_color(editor.config.theme.background);
    let prompt_background_color = convert_color(editor.config.theme.token_whitespace);
    let prompt_foreground_color = background_color;
    let cursor_color = convert_color(editor.config.theme.cursor);

    if has_focus {
        write_command(write, SetBackgroundColor(background_color));
        write_command(write, SetForegroundColor(foreground_color));
    } else {
        write_command(write, SetBackgroundColor(foreground_color));
        write_command(write, SetForegroundColor(background_color));
    }

    let x = if has_focus {
        let (status_message_kind, status_message) = editor.status_bar.message();
        let status_message = status_message.trim();

        if status_message.is_empty() {
            match editor.mode.kind() {
                ModeKind::Normal => match editor.recording_macro {
                    Some(key) => {
                        let text = "recording macro ";
                        let key = key.to_char();
                        write_command(write, Print(text));
                        write_command(write, Print(key));
                        Some(text.len() + 1)
                    }
                    None => Some(0),
                },
                ModeKind::Insert => {
                    let text = "-- INSERT --";
                    write_command(write, Print(text));
                    Some(text.len())
                }
                ModeKind::Picker | ModeKind::ReadLine | ModeKind::Script => {
                    let read_line = &editor.read_line;

                    write_command(write, SetBackgroundColor(prompt_background_color));
                    write_command(write, SetForegroundColor(prompt_foreground_color));
                    write_command(write, Print(read_line.prompt()));
                    write_command(write, SetBackgroundColor(background_color));
                    write_command(write, SetForegroundColor(foreground_color));
                    write_command(write, Print(read_line.input()));
                    write_command(write, SetBackgroundColor(cursor_color));
                    write_command(write, Print(' '));
                    write_command(write, SetBackgroundColor(background_color));
                    None
                }
            }
        } else {
            fn print_line<W>(write: &mut W, line: &str)
            where
                W: Write,
            {
                for c in line.chars() {
                    match c {
                        '\t' => write_command(write, Print("    ")),
                        c => write_command(write, Print(c)),
                    };
                }
            }

            let prefix = match status_message_kind {
                StatusMessageKind::Info => "",
                StatusMessageKind::Error => "error:",
            };

            let line_count = status_message.lines().count();
            if line_count > 1 {
                if prefix.is_empty() {
                    write_command(write, cursor::MoveUp((line_count - 1) as _));
                } else {
                    write_command(write, cursor::MoveUp(line_count as _));
                    write_command(write, SetBackgroundColor(prompt_background_color));
                    write_command(write, SetForegroundColor(prompt_foreground_color));
                    write_command(write, Print(prefix));
                    write_command(write, terminal::Clear(terminal::ClearType::UntilNewLine));
                    write_command(write, cursor::MoveToNextLine(1));
                    write_command(write, SetBackgroundColor(background_color));
                    write_command(write, SetForegroundColor(foreground_color));
                }

                for (i, line) in status_message.lines().enumerate() {
                    print_line(write, line);
                    if i < line_count - 1 {
                        write_command(write, terminal::Clear(terminal::ClearType::UntilNewLine));
                        write_command(write, cursor::MoveToNextLine(1));
                    }
                }
            } else {
                write_command(write, terminal::Clear(terminal::ClearType::CurrentLine));
                write_command(write, SetBackgroundColor(prompt_background_color));
                write_command(write, SetForegroundColor(prompt_foreground_color));
                write_command(write, Print(prefix));
                write_command(write, SetBackgroundColor(background_color));
                write_command(write, SetForegroundColor(foreground_color));
                print_line(write, status_message);
            }

            None
        }
    } else {
        Some(0)
    };

    let buffer_needs_save;
    let buffer_path;
    match client_view.buffer {
        Some(buffer) => {
            buffer_needs_save = buffer.needs_save();
            buffer_path = buffer.path().and_then(|p| p.to_str()).unwrap_or("");
        }
        None => {
            buffer_needs_save = false;
            buffer_path = "<no buffer>";
        }
    };

    buf.clear();
    match x {
        Some(x) => {
            use std::fmt::Write;

            let param_count = match editor.mode.kind() {
                ModeKind::Normal if has_focus => editor.mode.normal_state.count,
                _ => 0,
            };

            if has_focus {
                if param_count > 0 {
                    let _ = write!(buf, "{}", param_count);
                };
                for key in editor.buffered_keys.as_slice() {
                    let _ = write!(buf, "{}", key);
                }
                buf.push(' ');
            }

            let title_start = buf.len();
            if buffer_needs_save {
                buf.push('*');
            }
            buf.push_str(buffer_path);
            write_command(write, terminal::SetTitle(&buf[title_start..]));

            if client_view.buffer.is_some() {
                let line_number = client_view.main_cursor_position.line_index + 1;
                let column_number = client_view.main_cursor_position.column_byte_index + 1;
                let _ = write!(buf, ":{},{}", line_number, column_number);
            }
            buf.push(' ');

            let available_width = client_view.client.viewport_size.0 as usize - x;

            let min_index = buf.len() - buf.len().min(available_width);
            let min_index = buf
                .char_indices()
                .map(|(i, _)| i)
                .filter(|i| *i >= min_index)
                .next()
                .unwrap_or(buf.len());
            let buf = &buf[min_index..];

            for _ in 0..(available_width - buf.len()) {
                write_command(write, Print(' '));
            }
            write_command(write, Print(buf));
        }
        None => {
            if buffer_needs_save {
                buf.push('*');
            }
            buf.push_str(buffer_path);
            write_command(write, terminal::SetTitle(&buf));
        }
    }

    write_command(write, terminal::Clear(terminal::ClearType::UntilNewLine));
}
