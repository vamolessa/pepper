use std::{io, iter};

use crate::{
    buffer::CharDisplayDistances,
    buffer_position::{BufferPosition, BufferPositionIndex, BufferRange},
    buffer_view::{BufferViewHandle, CursorMovementKind},
    editor::Editor,
    editor_utils::StatusBarDisplay,
    mode::ModeKind,
    syntax::{Token, TokenKind},
    theme::Color,
};

pub static ENTER_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1b[?1049h";
pub static EXIT_ALTERNATE_BUFFER_CODE: &[u8] = b"\x1b[?1049l";
pub static HIDE_CURSOR_CODE: &[u8] = b"\x1b[?25l";
pub static SHOW_CURSOR_CODE: &[u8] = b"\x1b[?25h";
pub static RESET_STYLE_CODE: &[u8] = b"\x1b[0;49m";
pub static MODE_256_COLORS_CODE: &[u8] = b"\x1b[=19h";
pub static BEGIN_TITLE_CODE: &[u8] = b"\x1b]0;";
pub static END_TITLE_CODE: &[u8] = b"\x07";

static TOO_LONG_PREFIX: &[u8] = b"...";

pub fn clear_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[2K");
}

pub fn clear_until_new_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[0K");
}

pub fn move_cursor_to(buf: &mut Vec<u8>, x: usize, y: usize) {
    use io::Write;
    let _ = write!(buf, "\x1b[{};{}H", x, y);
}

pub fn move_cursor_to_next_line(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[1E");
}

pub fn move_cursor_up(buf: &mut Vec<u8>, count: usize) {
    use io::Write;
    let _ = write!(buf, "\x1b[{}A", count);
}

pub fn set_background_color(buf: &mut Vec<u8>, color: Color) {
    use io::Write;
    let _ = write!(buf, "\x1b[48;2;{};{};{}m", color.0, color.1, color.2);
}

pub fn set_foreground_color(buf: &mut Vec<u8>, color: Color) {
    use io::Write;
    let _ = write!(buf, "\x1b[38;2;{};{};{}m", color.0, color.1, color.2);
}

pub fn set_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[4m");
}

pub fn set_not_underlined(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"\x1b[24m");
}

pub struct RenderContext<'a> {
    pub editor: &'a Editor,
    pub status_bar_display: &'a StatusBarDisplay<'a, 'a>,
    pub viewport_size: (u16, u16),
    pub scroll: BufferPositionIndex,
    pub has_focus: bool,
}

pub fn draw(ctx: &RenderContext, buffer_view_handle: Option<BufferViewHandle>, buf: &mut Vec<u8>) {
    draw_buffer_view(ctx, buffer_view_handle, buf);
    draw_picker(ctx, buf);
    draw_statusbar(ctx, buffer_view_handle, buf);
}

fn draw_empty_view(ctx: &RenderContext, buf: &mut Vec<u8>) {
    move_cursor_to(buf, 0, 0);
    buf.extend_from_slice(RESET_STYLE_CODE);
    set_background_color(buf, ctx.editor.theme.background);
    set_foreground_color(buf, ctx.editor.theme.token_whitespace);

    let message_lines = &[
        concat!(env!("CARGO_PKG_NAME"), " editor"),
        concat!("version ", env!("CARGO_PKG_VERSION")),
        "",
        "type `:open <file-name><enter>` to edit a file",
        "or `:help<enter>` for help",
        "or `:help changelog<enter>` for the changelog",
    ];

    let width = ctx.viewport_size.0 as usize;
    let height = ctx.viewport_size.1.saturating_sub(1) as usize;

    let margin_top = (height.saturating_sub(message_lines.len())) / 2;
    let margin_bottom = height - margin_top - message_lines.len();

    let margin_bottom = if ctx.has_focus {
        let picker_height = ctx
            .editor
            .picker
            .len()
            .min(ctx.editor.config.picker_max_height as _);
        margin_bottom.saturating_sub(picker_height)
    } else {
        margin_bottom
    };

    let mut visual_empty = [0; 4];
    let visual_empty = ctx
        .editor
        .config
        .visual_empty
        .encode_utf8(&mut visual_empty)
        .as_bytes();

    for _ in 0..margin_top {
        buf.extend_from_slice(visual_empty);
        clear_until_new_line(buf);
        move_cursor_to_next_line(buf);
    }

    for line in message_lines {
        buf.extend_from_slice(visual_empty);

        let margin_left = (width.saturating_sub(line.len())) / 2;
        buf.extend(std::iter::repeat(b' ').take(margin_left));
        buf.extend_from_slice(line.as_bytes());

        clear_until_new_line(buf);
        move_cursor_to_next_line(buf);
    }

    for _ in 0..margin_bottom {
        buf.extend_from_slice(visual_empty);
        clear_until_new_line(buf);
        move_cursor_to_next_line(buf);
    }
}

fn draw_buffer_view(
    ctx: &RenderContext,
    buffer_view_handle: Option<BufferViewHandle>,
    buf: &mut Vec<u8>,
) {
    let buffer_view_handle = match buffer_view_handle {
        Some(handle) => handle,
        None => {
            draw_empty_view(ctx, buf);
            return;
        }
    };

    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
    let cursors = &buffer_view.cursors[..];
    let active_line_index = buffer_view.cursors.main_cursor().position.line_index as usize;

    let tab_size = ctx.editor.config.tab_size.get();

    let draw_width = ctx.viewport_size.0 as usize;
    let draw_height = ctx.viewport_size.1.saturating_sub(1);
    let draw_height = if ctx.has_focus {
        let picker_height = ctx
            .editor
            .picker
            .len()
            .min(ctx.editor.config.picker_max_height as _);
        draw_height.saturating_sub(picker_height as _)
    } else {
        draw_height
    };

    let cursor_color = if ctx.has_focus {
        match ctx.editor.mode.kind() {
            ModeKind::Insert => ctx.editor.theme.insert_cursor,
            _ => match ctx.editor.mode.normal_state.movement_kind {
                CursorMovementKind::PositionAndAnchor => ctx.editor.theme.normal_cursor,
                CursorMovementKind::PositionOnly => ctx.editor.theme.select_cursor,
            },
        }
    } else {
        ctx.editor.theme.inactive_cursor
    };

    let cursors_end_index = cursors.len().saturating_sub(1);

    let buffer_content = buffer.content();
    let highlighted_buffer = buffer.highlighted();
    let search_ranges = buffer.search_ranges();
    let search_ranges_end_index = search_ranges.len().saturating_sub(1);

    let lints = buffer.lints.all();
    let lints_end_index = lints.len().saturating_sub(1);

    let mut scroll_offset = BufferPosition::zero();
    let mut scroll_padding_top = ctx.scroll as usize;
    for (line_index, display_len) in buffer_content.line_display_lens().iter().enumerate() {
        scroll_offset.line_index = line_index as _;

        if scroll_padding_top == 0 {
            break;
        }

        let line_height = 1 + display_len.total_len(tab_size) / draw_width;
        if line_height <= scroll_padding_top {
            scroll_padding_top -= line_height;
            continue;
        }

        let line = buffer_content.lines()[line_index].as_str();
        let target_display_len = (scroll_padding_top * draw_width) as _;
        for d in CharDisplayDistances::new(line, tab_size) {
            if d.distance >= target_display_len {
                let index = d.char_index as usize + d.char.len_utf8();
                scroll_offset.column_byte_index = index as _;
                break;
            }
        }

        break;
    }

    let mut current_cursor_index = cursors.len();
    let mut current_cursor_position = BufferPosition::zero();
    let mut current_cursor_range = BufferRange::zero();
    for (i, cursor) in cursors.iter().enumerate() {
        let range = cursor.to_range();
        if scroll_offset <= range.to {
            current_cursor_index = i;
            current_cursor_position = cursor.position;
            current_cursor_range = range;
            break;
        }
    }

    let mut current_search_range_index = search_ranges.len();
    let mut current_search_range = BufferRange::zero();
    for (i, &range) in search_ranges.iter().enumerate() {
        if scroll_offset < range.to {
            current_search_range_index = i;
            current_search_range = range;
            break;
        }
    }

    let mut current_lint_index = lints.len();
    let mut current_lint_range = BufferRange::zero();
    for (i, lint) in lints.iter().enumerate() {
        if scroll_offset < lint.range.to {
            current_lint_index = i;
            current_lint_range = lint.range;
            break;
        }
    }

    move_cursor_to(buf, 0, 0);
    set_background_color(buf, ctx.editor.theme.background);
    set_not_underlined(buf);

    let mut char_buf = [0; std::mem::size_of::<char>()];

    let mut visual_empty = [0; 4];
    let visual_empty = ctx
        .editor
        .config
        .visual_empty
        .encode_utf8(&mut visual_empty)
        .as_bytes();

    let mut visual_space = [0; 4];
    let visual_space = ctx
        .editor
        .config
        .visual_space
        .encode_utf8(&mut visual_space)
        .as_bytes();

    let mut visual_tab_first = [0; 4];
    let visual_tab_first = ctx
        .editor
        .config
        .visual_tab_first
        .encode_utf8(&mut visual_tab_first)
        .as_bytes();

    let mut visual_tab_repeat = [0; 4];
    let visual_tab_repeat = ctx
        .editor
        .config
        .visual_tab_repeat
        .encode_utf8(&mut visual_tab_repeat)
        .as_bytes();

    let mut lines_drawn_count = 0;
    for (line_index, line) in buffer_content
        .lines()
        .iter()
        .enumerate()
        .skip(scroll_offset.line_index as _)
    {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum DrawState {
            Token(TokenKind),
            Selection(TokenKind),
            Highlight,
            Cursor,
        }

        if lines_drawn_count == draw_height {
            break;
        }
        lines_drawn_count += 1;

        let line = &line.as_str()[scroll_offset.column_byte_index as usize..];
        let mut draw_state = DrawState::Token(TokenKind::Text);
        let mut was_inside_lint_range = false;
        let mut x = 0;
        let mut last_line_token = Token::default();
        let mut line_tokens = highlighted_buffer.line_tokens(line_index).iter();

        let background_color = if line_index == active_line_index as _ {
            ctx.editor.theme.active_line_background
        } else {
            ctx.editor.theme.background
        };

        set_background_color(buf, background_color);
        set_foreground_color(buf, ctx.editor.theme.token_text);

        for (char_index, c) in line.char_indices().chain(iter::once((line.len(), '\n'))) {
            let char_index = char_index + scroll_offset.column_byte_index as usize;
            let char_position = BufferPosition::line_col(line_index as _, char_index as _);

            let token_kind = if c.is_ascii_whitespace() {
                TokenKind::Whitespace
            } else {
                if !last_line_token.contains(char_index as _) {
                    while let Some(token) = line_tokens.next() {
                        if token.contains(char_index as _) {
                            last_line_token = token.clone();
                            break;
                        }
                    }
                }
                last_line_token.kind
            };

            let text_color = match token_kind {
                TokenKind::Keyword => ctx.editor.theme.token_keyword,
                TokenKind::Type => ctx.editor.theme.token_type,
                TokenKind::Symbol => ctx.editor.theme.token_symbol,
                TokenKind::Literal => ctx.editor.theme.token_literal,
                TokenKind::String => ctx.editor.theme.token_string,
                TokenKind::Comment => ctx.editor.theme.token_comment,
                TokenKind::Text => ctx.editor.theme.token_text,
                TokenKind::Whitespace => ctx.editor.theme.token_whitespace,
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

            while current_lint_range.to < char_position && current_lint_index < lints_end_index {
                current_lint_index += 1;
                current_lint_range = lints[current_lint_index].range;
            }
            let inside_lint_range =
                current_lint_range.from <= char_position && char_position < current_lint_range.to;

            if inside_lint_range != was_inside_lint_range {
                was_inside_lint_range = inside_lint_range;
                if inside_lint_range {
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
                    set_foreground_color(buf, background_color);
                }
            } else if inside_search_range {
                if draw_state != DrawState::Highlight {
                    draw_state = DrawState::Highlight;
                    set_background_color(buf, ctx.editor.theme.highlight);
                    set_foreground_color(buf, background_color);
                }
            } else if draw_state != DrawState::Token(token_kind) {
                draw_state = DrawState::Token(token_kind);
                set_background_color(buf, background_color);
                set_foreground_color(buf, text_color);
            }

            let previous_x = x;
            let previous_buf_len = buf.len();

            match c {
                '\n' => {
                    x += 1;
                    buf.push(b' ');
                }
                ' ' => {
                    x += 1;
                    buf.extend_from_slice(visual_space);
                }
                '\t' => {
                    x += tab_size as usize;

                    buf.extend_from_slice(visual_tab_first);
                    for _ in 0..tab_size - 1 {
                        buf.extend_from_slice(visual_tab_repeat);
                    }
                }
                _ => {
                    x += 1;
                    buf.extend_from_slice(c.encode_utf8(&mut char_buf).as_bytes());
                }
            }

            if x > ctx.viewport_size.0 as _ {
                x -= ctx.viewport_size.0 as usize;
                lines_drawn_count += 1;
                if lines_drawn_count > draw_height {
                    lines_drawn_count = draw_height;
                    buf.truncate(previous_buf_len);
                    x = previous_x;
                    break;
                }
            }
        }

        scroll_offset.column_byte_index = 0;
        set_background_color(buf, background_color);

        if x < ctx.viewport_size.0 as _ {
            clear_until_new_line(buf);
        }

        move_cursor_to_next_line(buf);
    }

    set_not_underlined(buf);
    set_background_color(buf, ctx.editor.theme.background);
    set_foreground_color(buf, ctx.editor.theme.token_whitespace);

    for _ in lines_drawn_count..draw_height {
        buf.extend_from_slice(visual_empty);
        clear_until_new_line(buf);
        move_cursor_to_next_line(buf);
    }
}

fn draw_picker(ctx: &RenderContext, buf: &mut Vec<u8>) {
    if !ctx.has_focus {
        return;
    }

    let cursor = ctx.editor.picker.cursor().unwrap_or(usize::MAX - 1);
    let scroll = ctx.editor.picker.scroll();

    let width = ctx.viewport_size.0 as _;
    let height = ctx
        .editor
        .picker
        .len()
        .min(ctx.editor.config.picker_max_height as _);

    let background_normal_color = ctx.editor.theme.statusbar_inactive_background;
    let background_selected_color = ctx.editor.theme.statusbar_active_background;
    let foreground_color = ctx.editor.theme.token_text;

    set_background_color(buf, background_normal_color);
    set_foreground_color(buf, foreground_color);

    for (i, entry) in ctx
        .editor
        .picker
        .entries(&ctx.editor.word_database)
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
        x = 0;

        if x < width {
            clear_until_new_line(buf);
        }
        move_cursor_to_next_line(buf);
    }
}

fn draw_statusbar(
    ctx: &RenderContext,
    buffer_view_handle: Option<BufferViewHandle>,
    buf: &mut Vec<u8>,
) {
    let view_name;
    let needs_save;
    let main_cursor_position;
    let search_ranges;

    match buffer_view_handle {
        Some(handle) => {
            let buffer_view = ctx.editor.buffer_views.get(handle);
            let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);

            view_name = buffer.path.to_str().unwrap_or("");
            needs_save = buffer.needs_save();
            main_cursor_position = buffer_view.cursors.main_cursor().position;
            search_ranges = buffer.search_ranges();
        }
        None => {
            view_name = "";
            needs_save = false;
            main_cursor_position = BufferPosition::zero();
            search_ranges = &[];
        }
    }

    use io::Write;

    let background_active_color = ctx.editor.theme.statusbar_active_background;
    let background_innactive_color = ctx.editor.theme.statusbar_inactive_background;
    let foreground_color = ctx.editor.theme.token_text;
    let cursor_color = ctx.editor.theme.normal_cursor;

    if ctx.has_focus {
        set_background_color(buf, background_active_color);
    } else {
        set_background_color(buf, background_innactive_color);
    }
    set_foreground_color(buf, foreground_color);

    let x = if ctx.has_focus {
        let message_is_empty = ctx.status_bar_display.lines.is_empty();
        match ctx.editor.mode.kind() {
            ModeKind::Normal if message_is_empty => match ctx.editor.recording_macro {
                Some(key) => {
                    let text = b"recording macro ";
                    let key = key.as_u8();
                    buf.extend_from_slice(text);
                    buf.push(key);
                    Some(text.len() + 1)
                }
                None => match search_ranges {
                    [] => Some(0),
                    _ => {
                        let previous_len = buf.len();
                        let search_index = ctx.editor.mode.normal_state.search_index + 1;
                        let _ = write!(buf, " [{}/{}]", search_index, search_ranges.len());
                        Some(buf.len() - previous_len)
                    }
                },
            },
            ModeKind::Insert if message_is_empty => {
                let text = b"-- INSERT --";
                buf.extend_from_slice(text);
                Some(text.len())
            }
            ModeKind::Command | ModeKind::Picker | ModeKind::ReadLine => {
                let read_line = &ctx.editor.read_line;

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
            _ => {
                let line_count = ctx.status_bar_display.lines.len()
                    + ctx.status_bar_display.prefix_is_line as usize;
                if line_count > 1 {
                    move_cursor_up(buf, (line_count - 1) as _);
                }

                let prefix = ctx.status_bar_display.prefix.as_bytes();
                if !prefix.is_empty() {
                    set_background_color(buf, background_innactive_color);
                    set_foreground_color(buf, foreground_color);
                    buf.extend_from_slice(prefix);

                    if ctx.status_bar_display.prefix_is_line {
                        clear_until_new_line(buf);
                        move_cursor_to_next_line(buf);
                    }

                    set_background_color(buf, background_active_color);
                    set_foreground_color(buf, foreground_color);
                }

                if let Some((first, rest)) = ctx.status_bar_display.lines.split_first() {
                    buf.extend_from_slice(first.as_bytes());
                    clear_until_new_line(buf);
                    for &line in rest {
                        move_cursor_to_next_line(buf);
                        buf.extend_from_slice(line.as_bytes());
                        clear_until_new_line(buf);
                    }
                }

                None
            }
        }
    } else {
        Some(0)
    };

    if let Some(x) = x {
        fn take_chars(s: &str, char_count: usize) -> (usize, &str) {
            match s.char_indices().rev().enumerate().take(char_count).last() {
                Some((char_index, (byte_index, _))) => (char_index + 1, &s[byte_index..]),
                None => (0, s),
            }
        }

        let available_width = ctx.viewport_size.0 as usize - x;
        let half_available_width = available_width / 2;

        let status_start_index = buf.len();

        if ctx.has_focus {
            let param_count = ctx.editor.mode.normal_state.count;
            if param_count > 0 && matches!(ctx.editor.mode.kind(), ModeKind::Normal) {
                let _ = write!(buf, "{}", param_count);
            }
            for key in ctx.editor.buffered_keys.as_slice() {
                let _ = write!(buf, "{}", key);
            }
            buf.push(b' ');
        }

        if needs_save {
            buf.push(b'*');
        }

        let (char_count, view_name) = take_chars(view_name, half_available_width);
        if char_count == half_available_width {
            buf.extend_from_slice(TOO_LONG_PREFIX);
        }
        buf.extend_from_slice(view_name.as_bytes());

        if !view_name.is_empty() {
            let line_number = main_cursor_position.line_index + 1;
            let column_number = main_cursor_position.column_byte_index + 1;
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
            let (prefix, rest) = buf[start_index..].split_at_mut(TOO_LONG_PREFIX.len());
            prefix.copy_from_slice(TOO_LONG_PREFIX);
            for b in rest.iter_mut().take_while(|b| !b.is_ascii()) {
                *b = b'.';
            }
        }
    }

    buf.extend_from_slice(BEGIN_TITLE_CODE);
    if needs_save {
        buf.push(b'*');
    }
    if view_name.is_empty() {
        buf.extend_from_slice(b"no buffer");
    } else {
        buf.extend_from_slice(view_name.as_bytes());
    }
    buf.extend_from_slice(END_TITLE_CODE);

    clear_until_new_line(buf);
}
