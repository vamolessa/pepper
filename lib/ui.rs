use std::{io, iter};

use crate::{
    buffer::{Buffer, BufferContent, BufferHandle},
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::BufferViewHandle,
    cursor::Cursor,
    editor::Editor,
    editor_utils::MessageKind,
    mode::ModeKind,
    syntax::{HighlightedBuffer, TokenKind},
    theme::Color,
};

pub const ENTER_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1B[?1049h";
pub const EXIT_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1B[?1049l";
pub const HIDE_CURSOR_CODE: &[u8] = b"\x1B[?25l";
pub const SHOW_CURSOR_CODE: &[u8] = b"\x1B[?25h";
pub const RESET_STYLE_CODE: &[u8] = b"\x1B[0;49m";
pub const MODE_256_COLORS_CODE: &[u8] = b"\x1B[=19h";

#[inline]
pub fn set_title(buf: &mut Vec<u8>, title: &str) {
    buf.extend_from_slice(b"\x1B]0;");
    buf.extend_from_slice(title.as_bytes());
    buf.extend_from_slice(b"\x07");
}

#[inline]
pub fn clear_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[2K");
}

#[inline]
pub fn clear_until_new_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[0K");
}

#[inline]
pub fn move_cursor_to(buf: &mut Vec<u8>, x: usize, y: usize) {
    use io::Write;
    let _ = write!(buf, "\x1B[{};{}H", x, y);
}

#[inline]
pub fn move_cursor_to_next_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[1E");
}

#[inline]
pub fn move_cursor_up(buf: &mut Vec<u8>, count: usize) {
    use io::Write;
    let _ = write!(buf, "\x1B[{}A", count);
}

#[inline]
pub fn set_background_color(buf: &mut Vec<u8>, color: Color) {
    use io::Write;
    let _ = write!(buf, "\x1B[48;2;{};{};{}m", color.0, color.1, color.2);
}

#[inline]
pub fn set_foreground_color(buf: &mut Vec<u8>, color: Color) {
    use io::Write;
    let _ = write!(buf, "\x1B[38;2;{};{};{}m", color.0, color.1, color.2);
}

#[inline]
pub fn set_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[4m");
}

#[inline]
pub fn set_not_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1B[24m");
}

pub fn render(
    editor: &Editor,
    buffer_view_handle: Option<BufferViewHandle>,
    width: usize,
    height: usize,
    scroll: usize,
    has_focus: bool,
    buffer: &mut Vec<u8>,
    status_bar_buf: &mut String, // TODO: try to remove this
) {
    let view = View::new(editor, buffer_view_handle, width, height, scroll);

    draw_buffer(buffer, editor, &view, has_focus);
    if has_focus {
        draw_picker(buffer, editor, &view);
    }
    draw_statusbar(buffer, editor, &view, has_focus, status_bar_buf);
}

struct View<'a> {
    buffer_handle: Option<BufferHandle>,
    buffer: Option<&'a Buffer>,
    main_cursor_position: BufferPosition,
    cursors: &'a [Cursor],

    width: usize,
    height: usize,
    scroll: usize,
}

impl<'a> View<'a> {
    pub fn new(
        editor: &'a Editor,
        buffer_view_handle: Option<BufferViewHandle>,
        width: usize,
        height: usize,
        scroll: usize,
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

            width,
            height,
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

    let cursor_color = if has_focus && editor.mode.kind() == ModeKind::Insert {
        editor.theme.highlight
    } else {
        editor.theme.cursor
    };

    let mut text_color = editor.theme.token_text;

    move_cursor_to(buf, 0, 0);
    set_background_color(buf, editor.theme.background);
    set_foreground_color(buf, text_color);

    let mut line_index = view.scroll;
    let mut drawn_line_count = 0;

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

    // TODO: change to list of buffer linters (make lsp more like a plugin)
    // TODO: buffer_handle will not be needed, only a slice of 'Lints'
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

    let mut current_cursor_index = 0;
    let mut current_cursor_position = BufferPosition::default();
    let mut current_cursor_range = BufferRange::default();
    if let Some(cursor) = cursors.get(current_cursor_index) {
        current_cursor_position = cursor.position;
        current_cursor_range = cursor.to_range();
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

        set_foreground_color(buf, editor.theme.token_text);

        for (char_index, c) in line.as_str().char_indices().chain(iter::once((0, '\0'))) {
            if x >= view.width {
                move_cursor_to_next_line(buf);

                drawn_line_count += 1;
                x -= view.width;

                if drawn_line_count >= view.height {
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
                TokenKind::Keyword => editor.theme.token_keyword,
                TokenKind::Type => editor.theme.token_type,
                TokenKind::Symbol => editor.theme.token_symbol,
                TokenKind::Literal => editor.theme.token_literal,
                TokenKind::String => editor.theme.token_string,
                TokenKind::Comment => editor.theme.token_comment,
                TokenKind::Text => editor.theme.token_text,
                TokenKind::Whitespace => editor.theme.token_whitespace,
            };

            if current_cursor_range.to < char_position && current_cursor_index < cursors_end_index {
                current_cursor_index += 1;
                let cursor = cursors[current_cursor_index];
                current_cursor_position = cursor.position;
                current_cursor_range = cursor.to_range();
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

            match c {
                '\0' => {
                    buf.push(b' ');
                    x += 1;
                }
                ' ' => {
                    buf.push(editor.config.visual_space);
                    x += 1;
                }
                '\t' => {
                    buf.push(editor.config.visual_tab_first);
                    let tab_size = editor.config.tab_size.get() as usize;
                    let next_tab_stop = (tab_size - 1) - x % tab_size;
                    for _ in 0..next_tab_stop {
                        buf.push(editor.config.visual_tab_repeat);
                    }
                    x += next_tab_stop + 1;
                }
                _ => {
                    buf.extend_from_slice(c.encode_utf8(&mut char_buf).as_bytes());
                    x += 1;
                }
            }

            column_byte_index += c.len_utf8();
        }

        if x < view.width {
            set_background_color(buf, editor.theme.background);
            clear_until_new_line(buf);
        }

        move_cursor_to_next_line(buf);

        line_index += 1;
        drawn_line_count += 1;

        if drawn_line_count >= view.height {
            break;
        }
    }

    set_background_color(buf, editor.theme.background);
    set_foreground_color(buf, editor.theme.token_whitespace);
    for _ in drawn_line_count..view.height {
        buf.push(editor.config.visual_empty);
        clear_until_new_line(buf);
        move_cursor_to_next_line(buf);
    }
}

fn draw_picker(buf: &mut Vec<u8>, editor: &Editor, view: &View) {
    let cursor = editor.picker.cursor();
    let scroll = editor.picker.scroll();

    let half_width = view.width / 2;
    let half_width = half_width.saturating_sub(1) as usize;

    let height = editor.picker.height(editor.config.picker_max_height as _);

    let background_color = editor.theme.token_text;
    let foreground_color = editor.theme.token_whitespace;

    if height > 0 {
        move_cursor_up(buf, height);
    }

    set_background_color(buf, background_color);
    set_foreground_color(buf, foreground_color);

    for (i, entry) in editor
        .picker
        .entries(&editor.word_database, &editor.commands)
        .enumerate()
        .skip(scroll)
        .take(height)
    {
        if i == cursor {
            set_background_color(buf, foreground_color);
            set_foreground_color(buf, background_color);
        } else if i == cursor + 1 {
            set_background_color(buf, background_color);
            set_foreground_color(buf, foreground_color);
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

        let name_char_count = entry.name.chars().count();
        if name_char_count < half_width {
            for c in entry.name.chars() {
                print_char(buf, &mut x, c);
            }
        } else {
            buf.extend_from_slice(b"...");
            x += 3;
            let name_char_count = name_char_count + 3;
            for c in entry
                .name
                .chars()
                .skip(name_char_count.saturating_sub(half_width))
            {
                print_char(buf, &mut x, c);
            }
        }
        for _ in x..half_width {
            buf.push(b' ');
        }
        buf.push(b'|');
        x = 0;
        for c in entry.description.chars() {
            if x + 3 > half_width {
                buf.extend_from_slice(b"...");
                break;
            }
            print_char(buf, &mut x, c);
        }

        if x < view.width {
            clear_until_new_line(buf);
        }
        move_cursor_to_next_line(buf);
    }
}

fn draw_statusbar(
    buf: &mut Vec<u8>,
    editor: &Editor,
    view: &View,
    has_focus: bool,
    status_bar_buf: &mut String,
) {
    let background_color = editor.theme.token_text;
    let foreground_color = editor.theme.background;
    let prompt_background_color = editor.theme.token_whitespace;
    let prompt_foreground_color = background_color;
    let cursor_color = editor.theme.cursor;

    if has_focus {
        set_background_color(buf, background_color);
        set_foreground_color(buf, foreground_color);
    } else {
        set_background_color(buf, foreground_color);
        set_foreground_color(buf, background_color);
    }

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

                    set_background_color(buf, prompt_background_color);
                    set_foreground_color(buf, prompt_foreground_color);
                    buf.extend_from_slice(read_line.prompt().as_bytes());
                    set_background_color(buf, background_color);
                    set_foreground_color(buf, foreground_color);
                    buf.extend_from_slice(read_line.input().as_bytes());
                    set_background_color(buf, cursor_color);
                    buf.push(b' ');
                    set_background_color(buf, background_color);
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
                    set_background_color(buf, prompt_background_color);
                    set_foreground_color(buf, prompt_foreground_color);
                    buf.extend_from_slice(prefix);
                    clear_until_new_line(buf);
                    move_cursor_to_next_line(buf);
                    set_background_color(buf, background_color);
                    set_foreground_color(buf, foreground_color);
                }

                for (i, line) in message.lines().enumerate() {
                    let len = print_line(buf, line);
                    if i < line_count - 1 {
                        if len < view.width {
                            clear_until_new_line(buf);
                        }
                        move_cursor_to_next_line(buf);
                    }
                }
            } else {
                clear_line(buf);
                set_background_color(buf, prompt_background_color);
                set_foreground_color(buf, prompt_foreground_color);
                buf.extend_from_slice(prefix);
                set_background_color(buf, background_color);
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
            buffer_path = buffer.path().and_then(|p| p.to_str()).unwrap_or("");
        }
        None => {
            buffer_needs_save = false;
            buffer_path = "<no buffer>";
        }
    };

    status_bar_buf.clear();
    match x {
        Some(x) => {
            use std::fmt::Write;

            let param_count = match editor.mode.kind() {
                ModeKind::Normal if has_focus => editor.mode.normal_state.count,
                _ => 0,
            };

            if has_focus {
                if param_count > 0 {
                    let _ = write!(status_bar_buf, "{}", param_count);
                };
                for key in editor.buffered_keys.as_slice() {
                    let _ = write!(status_bar_buf, "{}", key);
                }
                status_bar_buf.push(' ');
            }

            let title_start = status_bar_buf.len();
            if buffer_needs_save {
                status_bar_buf.push('*');
            }
            status_bar_buf.push_str(buffer_path);
            set_title(buf, &status_bar_buf[title_start..]);

            if view.buffer.is_some() {
                let line_number = view.main_cursor_position.line_index + 1;
                let column_number = view.main_cursor_position.column_byte_index + 1;
                let _ = write!(status_bar_buf, ":{},{}", line_number, column_number);
            }
            status_bar_buf.push(' ');

            let available_width = view.width as usize - x;

            let min_index = status_bar_buf.len() - status_bar_buf.len().min(available_width);
            let min_index = status_bar_buf
                .char_indices()
                .map(|(i, _)| i)
                .filter(|i| *i >= min_index)
                .next()
                .unwrap_or(status_bar_buf.len());
            let status_bar_buf = &status_bar_buf[min_index..];

            for _ in 0..(available_width - status_bar_buf.len()) {
                buf.push(b' ');
            }
            buf.extend_from_slice(status_bar_buf.as_bytes());
        }
        None => {
            if buffer_needs_save {
                status_bar_buf.push('*');
            }
            status_bar_buf.push_str(buffer_path);
            set_title(buf, &status_bar_buf);
        }
    }

    clear_until_new_line(buf);
}
