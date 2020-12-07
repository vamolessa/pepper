# scripting api

### !!! UNDER CONSTRUCTION! These may change at any time !!!

These are builtin functions that can be used to config, automate and extend the editor.
Also, parameterless functions can be called without parenthesis if they're the sole expression being evaluated in script mode
(e.g. if you write `client.quit` it will be called as if you typed the trailing `()`).

If a parameter type ends with a `?`, it means that it is optional.
If a return type ends with a `?`, it means that it may be `nil`.

If a function has an alias, it means that it can be invoked by that name with the same parameters
however without any namespace prefix.

## client

#### `client.index() -> integer`
The index of current client. Index `0` is the 'server client' where main logic executes.

#### `client.current_buffer_view_handle(client_index: integer?)`
A client's current buffer view handle or current client's.

#### `client.quit()`
Quits current client if it's not the server or there are no unsaved buffers.

Alias: `q`

#### `client.force_quit()`
Force quit current client even if it's the server or there were unsaved buffers.

Alias: `fq`

#### `client.quit_all()`
Quits all connected clients if there are no unsaved buffers.

Alias: `qa`

#### `client.force_quit_all()`
Force quit all connected clients even if there were unsaved buffers.

Alias: `fqa`

## editor

#### `editor.version() -> string`
Returns the Pepper version string formatted as `major.minor.patch`.

#### `editor.os() -> string`
Returns the os name Pepper was compiled on.
Possible values: https://doc.rust-lang.org/std/env/consts/constant.OS.html

#### `editor.print(value: any)`
Prints a value to the editor's status bar.

Alias: `print`

## buffer

#### `buffer.all_handles() -> [integer]`
Returns an array with the handles of all opened buffers.

#### `buffer.line_count(buffer_handle: integer?) -> integer`
Returns the line count for buffer `buffer_handle` or the current one.

#### `buffer.line_at(line_index: integer, buffer_handle: integer?) -> string`
Returns the line at zero-based index `line_index` from buffer `buffer_handle` or the current one.

#### `buffer.path(buffer_handle: integer?) -> string`
Returns the path from buffer `buffer_handle` or the current one.

#### `buffer.path_matches(glob: Glob, buffer_handle: integer?) -> bool`
Returns true if buffer's path matches the given glob. False otherwise.

#### `buffer.needs_save(buffer_handle: integer?) -> bool`
Returns true if buffer has pending changed that were not yet saved to its file.

#### `buffer.set_search(search: string, buffer_handle: integer?)`
Highlight's all search matches inside buffer `buffer_handle` or the current one.
The search uses basic string matching using 'smart case'.
That is if `search` is all lowercase it performs an case insensitive search, and an case sensitive search otherwise.

#### `buffer.open(path: string)`
Tries to find an already opened buffer with the same path and set that one as this client's current buffer.
If there's none, it tries to load the buffer from the filesystem.

Alias: `o`

#### `buffer.close(buffer_handle: integer?)`
Closes buffer `buffer_handle` or the current one if it does not need saving.

Alias: `c`

#### `buffer.force_close(buffer_handle: integer?)`
Force close buffer `buffer_handle` or the current one even if it needs saving.

Alias: `fc`

#### `buffer.close_all()`
Closes all opened buffers if none of them needs saving.

Alias: `ca`

#### `buffer.force_close_all()`
Force close all opened buffers even if any of them needs saving.

Alias: `fca`

#### `buffer.save(path: string?, buffer_handle: integer?)`
Saves any pending buffer changes to the filesystem. Optionally, you can change the buffer's path to `path` before saving.

Alias: `s`

#### `buffer.save_all()`
Saves any pending changes from all opened buffers to the filesystem.

Alias: `sa`

#### `buffer.reload(buffer_handle: integer?)`
Reloads buffer's content from the filesystem if it does not need saving.

Alias: `r`

#### `buffer.force_reload(buffer_handle: integer?)`
Force reload buffer's content from the filesystem even if it needs saving.

Alias: `fr`

#### `buffer.reload_all()`
Reload all opened buffer's content from the filesystem if none of them needs saving.

Alias: `ra`

#### `buffer.force_reload_all()`
Force reload all opened buffer's content from the filesystem even if any of them needs saving.

Alias: `fra`

#### `buffer.commit_edits(buffer_handle: integer?)`
Commit all edits made to the buffer so far as an undo group to the history system.

#### `buffer.on_open(callback: fn(buffer_handle: integer))`
Add a new `callback` that will be called whenever a new buffer is opened.

#### `buffer.on_save(callback: fn(buffer_handle: integer))`
Add a new `callback` that will be called whenever a buffer is saved.

#### `buffer.on_close(callback: fn(buffer_handle: integer))`
Add a new `callback` that will be called whenever a new buffer is closed.
Note that you can only use `closed_buffer` functions on this `buffer_handle`.

## closed_buffer

#### `closed_buffer.path(buffer_handle: integer?) -> string`
Returns the path from the closed buffer `buffer_handle` or the recently closed one.

#### `closed_buffer.path_matches(glob: Glob, buffer_handle: integer?) -> bool`
Returns true if recently closed buffer's path matches the given glob. False otherwise.

## buffer_view

#### `buffer_view.buffer_handle(buffer_view_handle: integer?) -> integer
Returns the buffer handle associated with the view `buffer_view_handle` or the current one.
Returns `nil` if there is no buffer associated.

#### `buffer_view.all_handles() -> [integer]`
Returns an array with the handles of all the buffer views.

#### `buffer_view.handle_from_path(path: string) -> integer?`
If there's an opened buffer view which associated buffer has path equals to `path`, returns that buffer views' handle.
Returns `nil` if there was no such buffer view.

#### `buffer_view.selection_text(buffer_view_handle: integer?) -> string`
Returns current selected text from buffer view `buffer_view_handle` or the current one.

#### `buffer_view.insert_text(text: string, buffer_view_handle: integer?)`
Inserts text `text` at all cursor positions in buffer view `buffer_view_handle` or the current one.

#### `buffer_view.insert_text_at(text: string, line: integer, column: integer, buffer_view_handle: integer?)`
Inserts text `text` at zero-based position (`line`, `column`) in buffer view `buffer_view_handle` or the current one.

#### `buffer_view.delete_selection(buffer_view_handle: integer?)`
Deletes all selected text in buffer view `buffer_view_handle` or the current one.

#### `buffer_view.delete_in(from_line: integer, from_column: integer, to_line: integer, to_column: integer, buffer_view_handle: integer?)`
Deletes all text inside zero-based range from (`from_line`, `from_column`) to (`to_line`, `to_column`)
in buffer view `buffer_view_handle` or the current one.

#### `buffer_view.undo(buffer_view_handle: integer?)`
Undo changes made to buffer view `buffer_view_handle` or the current one.

#### `buffer_view.redo(buffer_view_handle: integer?)`
Redo changes made to buffer view `buffer_view_handle` or the current one.

## cursors

#### `Cursor`

A cursor object.
It always have the following members:

key | type
--- | ---
`anchor_line` | `integer`
`anchor_column` | `integer`
`position_line` | `integer`
`position_column` | `integer`

Anchor is the selection anchor that either never moves or follows the cursor position.

#### `cursors.len(buffer_view_handle: integer?) -> integer?`
Returns how many cursors there are for buffer view `buffer_view_handle` or the current one.
Returns `nil` if there was no buffer view.

#### `cursors.all(buffer_view_handle: integer?) -> [Cursor]`
Returns all the cursors in buffer view `buffer_view_handle` or the current one.
Returns an empty array if there was no buffer view.

#### `cursors.set_all(cursors: [Cursor], buffer_view_handle: integer?)`
Overrides all cursors for buffer view `buffer_view_handle` or the current one.
Buffer views always have at least one cursor, so if an empty array is passed, a cursor at line `0` and column `0` is added.

#### `cursors.main_index(buffer_view_handle: integer?) -> integer?`
Returns the zero-based index of the main cursor of the buffer view `buffer_view_handle` or the current one.
Main cursor is the one that dictates the buffer scrolling and is always visible on screen.
Returns `nil` if there was no buffer view.

#### `cursors.main(buffer_view_handle: integer?) -> Cursor?`
Returns the main cursor of the buffer view `buffer_view_handle` or the current one.
Main cursor is the one that dictates the buffer scrolling and is always visible on screen.
Returns `nil` if there was no buffer view.

#### `cursors.get(index: integer, buffer_view_handle: integer?) -> Cursor?`
Returns the cursor at zero-based index `index` of the buffer view `buffer_view_handle` or the current one.
Returns `nil` if there was no buffer view.

#### `cursors.set(index: integer, cursor: Cursor, buffer_view_handle: integer?)`
Sets the cursor at zero-based index `index` of the buffer view `buffer_view_handle` or the current one.

#### `cursor.move_columns(count: integer, selecting: bool, buffer_view_handle: integer?)`
Move all cursors of buffer view `buffer_view_handle` or the current one by `count` columns.
If `selecting` is true, it won't move cursors' anchors.

#### `cursor.move_lines(count: integer, selecting: bool, buffer_view_handle: integer?)`
Move all cursors of buffer view `buffer_view_handle` or the current one by `count` lines.
If `selecting` is true, it won't move cursors' anchors.

#### `cursor.move_words(count: integer, selecting: bool, buffer_view_handle: integer?)`
Move all cursors of buffer view `buffer_view_handle` or the current one by `count` words.
If `selecting` is true, it won't move cursors' anchors.

#### `cursor.move_home(selecting: bool, buffer_view_handle: integer?)`
Move all cursors of buffer view `buffer_view_handle` or the current one to the first column of their line.
If `selecting` is true, it won't move cursors' anchors.

#### `cursor.move_end(selecting: bool, buffer_view_handle: integer?)`
Move all cursors of buffer view `buffer_view_handle` or the current one to the last column of their line.
If `selecting` is true, it won't move cursors' anchors.

#### `cursor.move_first_line(selecting: bool, buffer_view_handle: integer?)`
Move all cursors of buffer view `buffer_view_handle` or the current one to the first buffer line.
If `selecting` is true, it won't move cursors' anchors.

#### `cursor.move_last_line(selecting: bool, buffer_view_handle: integer?)`
Move all cursors of buffer view `buffer_view_handle` or the current one to the last buffer line.
If `selecting` is true, it won't move cursors' anchors.

## read_line

#### `read_line.prompt(text: string)`
Sets the prompt text that will appear when calling `read_line.read()` from now on.

#### `read_line.read(callback: fn(text: string?))`
Begins reading a line from the user.
If the user submits the line, `callback` is called with the text the user wrote.
If they cancels it, `callback` is called with `nil`.

## picker

#### `picker.prompt(text: string)`
Sets the prompt text that will appear when calling `picker.pick()` from now on.

#### `picker.entry(name: string, description: string?)`
Adds a new entry that will be shown in the next call to `picker.pick()`.
`description` is optional and it's considered an empty string if `nil`.

#### `picker.pick(callback: fn(name: string?, description: string?))`
Presents a picker ui for the user to select an option from the ones provided with `picker.entry()`.
If the user selects an option, `callback` is called with the `name` and `description` of the selected entry.
If they cancels it, `callback` is called with `nil`.

## process

#### `process.pipe(name: string, args: [string]?, stdin: string?) -> string, string, bool`
Attempts to invoke process `name` with optional arguments `args`.
If `stdin` is `nil`, `dev/null` is passed as stdin. Otherwise, its contents are passed as stdin.
This call blocks until the process exits.
It returns the process stdout, stderr and a boolean if the process exit with a success status code.

#### `process.spawn(name: string, args: [string]?, stdin: string?) -> string, string, bool`
Attempts to invoke process `name` with optional arguments `args`.
If `stdin` is `nil`, `dev/null` is passed as stdin. Otherwise, its contents are passed as stdin.
This call does not block, however it's not possible to get process output.

## keymap

#### `keymap.normal(from: string, to: string)`
Maps keys in `from` to keys in `to` when in normal mode.

#### `keymap.insert(from: string, to: string)`
Maps keys in `from` to keys in `to` when in insert mode.

## syntax

A `SyntaxRule` object may have these members:

key | type | doc
--- | --- | ---
`keyword` | `[Pattern]` | all keyword patterns
`symbol` | `[Pattern]` | all symbol patterns
`type` | `[Pattern]` | all type patterns
`literal` | `[Pattern]` | all literal patterns
`string` | `[Pattern]` | all string patterns
`comment` | `[Pattern]` | all comment patterns
`text` | `[Pattern]` | patterns that match regular text in a syntax

Note that syntax tokens are always matched in the order above.

#### `syntax.rules(glob: Glob, rules: SyntaxRules)`
Creates a new syntax that will be used when a buffer's path matches `glob`.
The syntax is defined by the `rules` object.

## glob

#### `glob.compile(pattern: string) -> Glob`
Compiles a pattern into a `Glob` object.

#### `glob.matches(glob: Glob, path: string) -> bool`
Returns true if `glob` matches `path`. False otherwise.

## config

`config` is a global object containing general config values.

key | type | doc
--- | --- | ---
`tab_size` | `integer` | size of a tab relative to space
`indent_with_tabs` | `bool` | if false, the editor will indent with `tab_size` spaces
`visual_empty` | `char` | the character that will be drawn to indicate end of buffer
`visual_space` | `char` | the character that will be drawn in place of spaces
`visual_tab_first` | `char` | the first character that will be drawn in place of a tab
`visual_tab_repeat` | `char` | the character that will be drawn repeatedly in place of a tab until we read a tab stop
`picker_max_height` | `integer` | max number of lines that are shown at a time when a picker ui is opened

## theme

`theme` is a global object containing theme color values.
All colors have type `integer` and are encoded as RGB hex values as `0xRRGGBB`.

key |  doc
--- | ---
`background` | The color displayed behind the characters on the screen
`highlight` | The color of search highlights that appear behind search matches. Also the cursor color while in insert mode
`cursor` | The cursor color while in any mode except insert mode
`token_whitespace` | All highlighted `whitespace` tokens have this color
`token_text` | All highlighted `text` tokens have this color
`token_comment` | All highlighted `comment` tokens have this color
`token_keyword` | All highlighted `keyword` tokens have this color
`token_type` | All highlighted `type` tokens have this color
`token_symbol` | All highlighted `symbol` tokens have this color
`token_string` | All highlighted `string` tokens have this color
`token_literal` | All highlighted `literal` tokens have this color

## registers

`registers` is a global object containing all editor registers values.
Registers are strings that are shared between the editor and the user.
`registers` is indexed by the register names which is a single lowercase letter string (that is `[a-z]`).

Here are some special registers:

register name | doc
--- | ---
`a` | Auto macro register. It contains the recorded keys for the macro that replays when you normaly press `.`
`k` | Key queue register. If you set this register with keys, they will be played as soon as you leave script mode
`s` | Search register. It contains the pattern of the last search performed. Setting it will perform a new search the next time you try to move to next search result

Note that when you record a macro, it will be store on the register of the key you press after `q`.