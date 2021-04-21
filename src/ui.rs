use std::{io, iter};

use crate::{
    buffer::{Buffer, BufferContent, BufferHandle},
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewHandle, CursorMovementKind},
    cursor::Cursor,
    editor::Editor,
    editor_utils::MessageKind,
    mode::ModeKind,
    syntax::{HighlightedBuffer, TokenKind},
    theme::Color,
};

pub const ENTER_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1b[?1049h";
pub const EXIT_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1b[?1049l";
pub const HIDE_CURSOR_CODE: &[u8] = b"\x1b[?25l";
pub const SHOW_CURSOR_CODE: &[u8] = b"\x1b[?25h";
pub const RESET_STYLE_CODE: &[u8] = b"\x1b[0;49m";
pub const MODE_256_COLORS_CODE: &[u8] = b"\x1b[=19h";
pub const BEGIN_TITLE_CODE: &[u8] = b"\x1b]0;";
pub const END_TITLE_CODE: &[u8] = b"\x07";

const TOO_LONG_PREFIX: &[u8] = b"...";

#[inline]
pub fn clear_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[2K");
}

#[inline]
pub fn clear_until_new_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[0K");
}

#[inline]
pub fn move_cursor_to(buf: &mut Vec<u8>, x: usize, y: usize) {
    use io::Write;
    let _ = write!(buf, "\x1b[{};{}H", x, y);
}

#[inline]
pub fn move_cursor_to_next_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[1E");
}

#[inline]
pub fn move_cursor_up(buf: &mut Vec<u8>, count: usize) {
    use io::Write;
    let _ = write!(buf, "\x1b[{}A", count);
}

#[inline]
pub fn set_background_color(buf: &mut Vec<u8>, color: Color) {
    use io::Write;
    let _ = write!(buf, "\x1b[48;2;{};{};{}m", color.0, color.1, color.2);
}

#[inline]
pub fn set_foreground_color(buf: &mut Vec<u8>, color: Color) {
    use io::Write;
    let _ = write!(buf, "\x1b[38;2;{};{};{}m", color.0, color.1, color.2);
}

#[inline]
pub fn set_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[4m");
}

#[inline]
pub fn set_not_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[24m");
}

pub fn render(
    editor: &Editor,
    buffer_view_handle: Option<BufferViewHandle>,
    size: (u16, u16),
    scroll: (u32, u32),
    has_focus: bool,
    buffer: &mut Vec<u8>,
) {
    let view = View::new(editor, buffer_view_handle, size, scroll);

    draw_buffer(buffer, editor, &view, has_focus);
    if has_focus {
        draw_picker(buffer, editor, &view);
    }
    draw_statusbar(buffer, editor, &view, has_focus);
}

struct View<'a> {
    buffer_handle: Option<BufferHandle>,
    buffer: Option<&'a Buffer>,
    main_cursor_position: BufferPosition,
    cursors: &'a [Cursor],

    size: (u16, u16),
    scroll: (u32, u32),
}

impl<'a> View<'a> {
    pub fn new(
        editor: &'a Editor,
        buffer_view_handle: Option<BufferViewHandle>,
        size: (u16, u16),
        scroll: (u32, u32),
    ) -> View<'a> {
        let buffer_view = buffer_view_handle.and_then(|h| editor.buffer_views.get(h));
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

        View {
            buffer_handle,
            buffer,
            main_cursor_position,
            cursors,
            size,
            scroll,
        }
    }
}

fn draw_buffer(buf: &mut Vec<u8>, editor: &Editor, view: &View, has_focus: bool) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum DrawState {
        Token(TokenKind),
        Selection(TokenKind),
        Highlight,
        Cursor,
    }

    let mut char_buf = [0; std::mem::size_of::<char>()];

    let cursor_color = if has_focus {
        match editor.mode.kind() {
            ModeKind::Insert => editor.theme.insert_cursor,
            _ => match editor.mode.normal_state.movement_kind {
                CursorMovementKind::PositionAndAnchor => editor.theme.normal_cursor,
                CursorMovementKind::PositionOnly => editor.theme.select_cursor,
            },
        }
    } else {
        editor.theme.inactive_cursor
    };

    let mut text_color = editor.theme.token_text;

    move_cursor_to(buf, 0, 0);
    set_background_color(buf, editor.theme.background);
    set_foreground_color(buf, text_color);
    set_not_underlined(buf);

    let cursors = &view.cursors[..];
    let cursors_end_index = cursors.len().saturating_sub(1);

    let buffer_content;
    let highlighted_buffer;
    let search_ranges;
    match view.buffer {
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

    let diagnostics = match view.buffer_handle {
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

    let display_position_offset = BufferPosition::line_col(view.scroll.1 as _, view.scroll.0 as _);

    let mut current_cursor_index = cursors.len();
    let mut current_cursor_position = BufferPosition::default();
    let mut current_cursor_range = BufferRange::default();
    for (i, cursor) in cursors.iter().enumerate() {
        let range = cursor.to_range();
        if display_position_offset <= range.to {
            current_cursor_index = i;
            current_cursor_position = cursor.position;
            current_cursor_range = range;
            break;
        }
    }

    let mut current_search_range_index = search_ranges.len();
    let mut current_search_range = BufferRange::default();
    for (i, &range) in search_ranges.iter().enumerate() {
        if display_position_offset < range.to {
            current_search_range_index = i;
            current_search_range = range;
            break;
        }
    }

    let mut current_diagnostic_index = diagnostics.len();
    let mut current_diagnostic_range = BufferRange::default();
    for (i, diagnostic) in diagnostics.iter().enumerate() {
        if display_position_offset < diagnostic.range.to {
            current_diagnostic_index = i;
            current_diagnostic_range = diagnostic.range;
            break;
        }
    }

    let mut lines_drawn_count = 0;
    for (line_index, line) in buffer_content
        .lines()
        .enumerate()
        .skip(view.scroll.1 as _)
        .take(view.size.1 as _)
    {
        lines_drawn_count += 1;

        let mut draw_state = DrawState::Token(TokenKind::Text);
        let mut was_inside_diagnostic_range = false;
        let mut x = 0;

        set_foreground_color(buf, editor.theme.token_text);

        let line = line.as_str();
        for (char_index, c) in line.char_indices().chain(iter::once((line.len(), '\n'))) {
            if char_index < view.scroll.0 as _ {
                continue;
            }

            let buf_len = buf.len();
            let char_position = BufferPosition::line_col(line_index, char_index);

            let token_kind = if c.is_ascii_whitespace() {
                TokenKind::Whitespace
            } else {
                highlighted_buffer.find_token_kind_at(line_index, char_index)
            };

            text_color = match token_kind {
                TokenKind::Keyword => editor.theme.token_keyword,
                TokenKind::Type => editor.theme.token_type,
                TokenKind::Symbol => editor.theme.token_symbol,
                TokenKind::Literal => editor.theme.token_literal,
                TokenKind::String => editor.theme.token_string,
                TokenKind::Comment => editor.theme.token_comment,
                TokenKind::Text => editor.theme.token_text,
                TokenKind::Whitespace => editor.theme.token_whitespace,
            };

            while current_cursor_index < cursors_end_index
                && current_cursor_range.to < char_position
            {
                current_cursor_index += 1;
                let cursor = cursors[current_cursor_index];
                current_cursor_position = cursor.position;
                current_cursor_range = cursor.to_range();
            }
            let inside_cursor_range = current_cursor_range.from <= char_position
                && char_position < current_cursor_range.to;

            while current_search_range.to <= char_position
                && current_search_range_index < search_ranges_end_index
            {
                current_search_range_index += 1;
                current_search_range = search_ranges[current_search_range_index];
            }
            let inside_search_range = current_search_range.from <= char_position
                && char_position < current_search_range.to;

            while current_diagnostic_range.to < char_position
                && current_diagnostic_index < diagnostics_end_index
            {
                current_diagnostic_index += 1;
                current_diagnostic_range = diagnostics[current_diagnostic_index].range;
            }
            let inside_diagnostic_range = current_diagnostic_range.from <= char_position
                && char_position < current_diagnostic_range.to;

            if inside_diagnostic_range != was_inside_diagnostic_range {
                was_inside_diagnostic_range = inside_diagnostic_range;
                if inside_diagnostic_range {
                    set_underlined(buf);
                } else {
                    set_not_underlined(buf);
                }
            }

            if char_position == current_cursor_position {
                if draw_state != DrawState::Cursor {
                    draw_state = DrawState::Cursor;
                    set_background_color(buf, cursor_color);
                    set_foreground_color(buf, text_color);
                }
            } else if inside_cursor_range {
                if draw_state != DrawState::Selection(token_kind) {
                    draw_state = DrawState::Selection(token_kind);
                    set_background_color(buf, text_color);
                    set_foreground_color(buf, editor.theme.background);
                }
            } else if inside_search_range {
                if draw_state != DrawState::Highlight {
                    draw_state = DrawState::Highlight;
                    set_background_color(buf, editor.theme.highlight);
                    set_foreground_color(buf, editor.theme.background);
                }
            } else if draw_state != DrawState::Token(token_kind) {
                draw_state = DrawState::Token(token_kind);
                set_background_color(buf, editor.theme.background);
                set_foreground_color(buf, text_color);
            }

            let previous_x = x;
            match c {
                '\n' => {
                    x += 1;
                    buf.push(b' ');
                }
                ' ' => {
                    x += 1;
                    buf.push(editor.config.visual_space);
                }
                '\t' => {
                    buf.push(editor.config.visual_tab_first);
                    let tab_size = editor.config.tab_size.get() as usize;
                    let next_tab_stop = (tab_size - 1) - x % tab_size;
                    x += next_tab_stop + 1;

                    for _ in 0..next_tab_stop {
                        buf.push(editor.config.visual_tab_repeat);
                    }
                }
                _ => {
                    x += 1;
                    buf.extend_from_slice(c.encode_utf8(&mut char_buf).as_bytes());
                }
            }

            if x > view.size.0 as _ {
                x = previous_x;
                buf.truncate(buf_len);
                break;
            }
        }

        if x < view.size.0 as _ {
            set_background_color(buf, editor.theme.background);
            clear_until_new_line(buf);
        }

        move_cursor_to_next_line(buf);
    }

    set_not_underlined(buf);
    set_background_color(buf, editor.theme.background);
    set_foreground_color(buf, editor.theme.token_whitespace);

    for _ in lines_drawn_count..view.size.1 {
        buf.push(editor.config.visual_empty);
        clear_until_new_line(buf);
        move_cursor_to_next_line(buf);
    }
}

fn draw_picker(buf: &mut Vec<u8>, editor: &Editor, view: &View) {
    let cursor = editor.picker.cursor().unwrap_or(usize::MAX - 1);
    let scroll = editor.picker.scroll();

    let width = view.size.0 as _;
    let height = editor
        .picker
        .len()
        .min(editor.config.picker_max_height as _);

    let background_normal_color = editor.theme.statusbar_inactive_background;
    let background_selected_color = editor.theme.statusbar_active_background;
    let foreground_color = editor.theme.token_text;

    set_background_color(buf, background_normal_color);
    set_foreground_color(buf, foreground_color);

    for (i, entry) in editor
        .picker
        .entries(&editor.word_database)
        .enumerate()
        .skip(scroll)
        .take(height)
    {
        if i == cursor {
            set_background_color(buf, background_selected_color);
        } else if i == cursor + 1 {
            set_background_color(buf, background_normal_color);
        }

        let mut x = 0;

        #[inline]
        fn print_char(buf: &mut Vec<u8>, x: &mut usize, c: char) {
            let mut char_buf = [0; std::mem::size_of::<char>()];

            *x += 1;
            match c {
                '\t' => buf.push(b' '),
                c => buf.extend_from_slice(c.encode_utf8(&mut char_buf).as_bytes()),
            }
        }

        let name_char_count = entry.chars().count();
        if name_char_count < width {
            for c in entry.chars() {
                print_char(buf, &mut x, c);
            }
        } else {
            buf.extend_from_slice(b"...");
            x += 3;
            let name_char_count = name_char_count + 3;
            for c in entry.chars().skip(name_char_count.saturating_sub(width)) {
                print_char(buf, &mut x, c);
            }
        }
        for _ in x..width {
            buf.push(b' ');
        }
        buf.push(b'|');
        x = 0;

        if x < width {
            clear_until_new_line(buf);
        }
        move_cursor_to_next_line(buf);
    }
}

fn draw_statusbar(buf: &mut Vec<u8>, editor: &Editor, view: &View, has_focus: bool) {
    let background_active_color = editor.theme.statusbar_active_background;
    let background_innactive_color = editor.theme.statusbar_inactive_background;
    let foreground_color = editor.theme.token_text;
    let cursor_color = editor.theme.normal_cursor;

    if has_focus {
        set_background_color(buf, background_active_color);
    } else {
        set_background_color(buf, background_innactive_color);
    }
    set_foreground_color(buf, foreground_color);

    let x = if has_focus {
        let (message_target, message) = editor.status_bar.message();
        let message = message.trim_end();

        if message.trim_start().is_empty() {
            match editor.mode.kind() {
                ModeKind::Normal => match editor.recording_macro {
                    Some(key) => {
                        let text = b"recording macro ";
                        let key = key.as_u8();
                        buf.extend_from_slice(text);
                        buf.push(key);
                        Some(text.len() + 1)
                    }
                    None => Some(0),
                },
                ModeKind::Insert => {
                    let text = b"-- INSERT --";
                    buf.extend_from_slice(text);
                    Some(text.len())
                }
                ModeKind::Command | ModeKind::Picker | ModeKind::ReadLine => {
                    let read_line = &editor.read_line;

                    set_background_color(buf, background_innactive_color);
                    set_foreground_color(buf, foreground_color);
                    buf.extend_from_slice(read_line.prompt().as_bytes());
                    set_background_color(buf, background_active_color);
                    set_foreground_color(buf, foreground_color);
                    buf.extend_from_slice(read_line.input().as_bytes());
                    set_background_color(buf, cursor_color);
                    buf.push(b' ');
                    set_background_color(buf, background_active_color);
                    None
                }
            }
        } else {
            fn print_line(buf: &mut Vec<u8>, line: &str) -> usize {
                let mut char_buf = [0; std::mem::size_of::<char>()];
                let mut len = 0;
                for c in line.chars() {
                    match c {
                        '\t' => {
                            buf.extend_from_slice(b"  ");
                            len += 2;
                        }
                        c => {
                            buf.extend_from_slice(c.encode_utf8(&mut char_buf).as_bytes());
                            len += 1;
                        }
                    };
                }
                len
            }

            let prefix = match message_target {
                MessageKind::Info => &[],
                MessageKind::Error => &b"error:"[..],
            };

            let line_count = message.lines().count();
            if line_count > 1 {
                if prefix.is_empty() {
                    move_cursor_up(buf, line_count - 1);
                } else {
                    move_cursor_up(buf, line_count);
                    set_background_color(buf, background_innactive_color);
                    set_foreground_color(buf, foreground_color);
                    buf.extend_from_slice(prefix);
                    clear_until_new_line(buf);
                    move_cursor_to_next_line(buf);
                    set_background_color(buf, background_active_color);
                    set_foreground_color(buf, foreground_color);
                }

                for (i, line) in message.lines().enumerate() {
                    let len = print_line(buf, line);
                    if i < line_count - 1 {
                        if len < view.size.0 as _ {
                            clear_until_new_line(buf);
                        }
                        move_cursor_to_next_line(buf);
                    }
                }
            } else {
                clear_line(buf);
                set_background_color(buf, background_innactive_color);
                set_foreground_color(buf, foreground_color);
                buf.extend_from_slice(prefix);
                set_background_color(buf, background_active_color);
                set_foreground_color(buf, foreground_color);
                print_line(buf, message);
            }

            None
        }
    } else {
        Some(0)
    };

    let buffer_needs_save;
    let buffer_path;
    match view.buffer {
        Some(buffer) => {
            buffer_needs_save = buffer.needs_save();
            buffer_path = buffer.path().to_str().unwrap_or("");
        }
        None => {
            buffer_needs_save = false;
            buffer_path = "<no buffer>";
        }
    };

    if let Some(x) = x {
        fn take_chars(s: &str, char_count: usize) -> (usize, &str) {
            match s.char_indices().enumerate().take(char_count).last() {
                Some((char_index, (byte_index, c))) => {
                    let count = char_index + 1;
                    if count < char_count {
                        (count, s)
                    } else {
                        let len = byte_index + c.len_utf8();
                        (count, &s[..len])
                    }
                }
                None => (0, s),
            }
        }

        use io::Write;

        let available_width = view.size.0 as usize - x;
        let half_available_width = available_width / 2;

        let status_start_index = buf.len();

        if has_focus {
            let param_count = editor.mode.normal_state.count;
            if param_count > 0 && matches!(editor.mode.kind(), ModeKind::Normal) {
                let _ = write!(buf, "{}", param_count);
            }
            for key in editor.buffered_keys.as_slice() {
                let _ = write!(buf, "{}", key);
            }
            buf.push(b' ');
        }

        if buffer_needs_save {
            buf.push(b'*');
        }

        let (char_count, buffer_path) = take_chars(buffer_path, half_available_width);
        if char_count == half_available_width {
            buf.extend_from_slice(TOO_LONG_PREFIX);
        }
        buf.extend_from_slice(buffer_path.as_bytes());

        if view.buffer.is_some() {
            let line_number = view.main_cursor_position.line_index + 1;
            let column_number = view.main_cursor_position.column_byte_index + 1;
            let _ = write!(buf, ":{},{}", line_number, column_number);
        }
        buf.push(b' ');

        let status = match std::str::from_utf8(&buf[status_start_index..]) {
            Ok(status) => status,
            Err(_) => {
                buf.truncate(status_start_index);
                return;
            }
        };

        let available_width_minus_prefix = available_width.saturating_sub(TOO_LONG_PREFIX.len());
        let (char_count, status) = take_chars(status, available_width_minus_prefix);

        let status_len = status.len();
        let buf_len = status_start_index + status_len + available_width - char_count;
        buf.resize(buf_len, 0);
        buf.copy_within(
            status_start_index..status_start_index + status_len,
            buf_len - status_len,
        );
        for b in &mut buf[status_start_index..buf_len - status_len] {
            *b = b' ';
        }
        if char_count == available_width_minus_prefix {
            let start_index = buf_len - status_len - TOO_LONG_PREFIX.len();
            buf[start_index..start_index + TOO_LONG_PREFIX.len()].copy_from_slice(TOO_LONG_PREFIX);
        }
    }

    buf.extend_from_slice(BEGIN_TITLE_CODE);
    if buffer_needs_save {
        buf.push(b'*');
    }
    buf.extend_from_slice(buffer_path.as_bytes());
    buf.extend_from_slice(END_TITLE_CODE);

    clear_until_new_line(buf);
}