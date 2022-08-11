These are the builtin commands that can be used to interact with the editor.
Their main purpose is to provide features that complement text editing.
Commands is also how you can configure your editor on config files.

When passing arguments that contain spaces, you can wrap them between `"`, `'` or a balanced `{}` pair.

# builtin commands

## `help`
Searches the help pages for `<keyword>`.
If `<keyword>` is not present, opens the main help page.
- default alias: `h`
- usage: `help [<keyword>]`

## `log`
Logs each `<argument>` to the editor log using the `<log-kind>`.
Each argument is separated by a new line.
Possible `<log-kind>`:
- `status`: will write a message only to the status bar
- `info`: will write a message only to both the log file status bar
- `diagnostic`: will write a message only to the log file
- `error`: will write an error message to both the log file and status bar

- usage: `log <log-kind> <arguments...>`

## `open-log`
Opens the editor log file as a buffer (if you want to refresh it, use the `reopen` command).
- usage: `open-log`

## `quit`
Quits this client.
With '!' will discard any unsaved changes.
- usage `quit[!]`
- default alias: `q`

## `quit-all`
Quits all clients.
With '!' will discard any unsaved changes.
- usage: `quit-all[!]`
- default alias: `qa`

## `open`
Opens buffer up for editting.
If file `<path>` exists, it will be loaded into the buffer's content.
Also, if `<path>` ends with `:<line>[,<column>]`, it will be opened at that location.

A buffer has a set of properties that can be changed when opening it:
- `history-enabled`, `history-disabled`: enables/disables undo history (enabled by default)
- `saving-enabled`, `saving-disabled`: enables/disables saving (enabled by default)
- `word-database-enabled`, `word-database-disabled`: enables/disables contributing words for the word database (builtin autocomplete) (enabled by default)

It's also possible to change these properties in batch by passing:
- `text`: will enable all properties
- `scratch`: will disable all properties

Note that the property evaluation order is the same as the order of the arguments.
That is, calling `open history-enabled scratch my-buffer.txt` will actually open `my-buffer.txt` with undo history disabled!

- usage: `open [<properties...>] <path>[:<line>[,<column>]]`
- default alias: `o`

## `save`
Saves buffer to file.
If `<path>` is present, it will use that path so save the buffer's content, making it the new buffer's associated filepath
(it will also enable saving for that buffer from now on).
- usage: `save [<path>]`
- default alias: `s`

## `save-all`
Saves all buffers to file.
- usage: `save-all`
- default alias: `sa`

## `reopen`
Reopens buffer from file. If it can not save, it does nothing.
With '!' will discard any unsaved changes.
- usage: `reopen[!]`
- default alias: `r`

## `reopen-all`
Reopens all buffers from file. Buffers that can not save, are skipped.
With '!' will discard any unsaved changes
- usage: `reopen-all[!]`
- default alias: `ra`

## `close`
Closes current buffer.
With '!' will discard any unsaved changes.
- usage: `close[!]`
- default alias: `c`

## `close-all`
Closes all buffers.
With '!' will discard any unsaved changes.
- usage: `close-all[!]`
- default alias: `ca`

## `config`
If `<value>` is present, it sets the editor config `<key>` to its value (if valid).
Otherwise, it returns its current value.
- usage: `config <key> [<value>]`

key | type | doc
--- | --- | ---
`tab_size` | `integer` | size of a tab relative to space (non zero)
`indent_with_tabs` | `bool` | if false, the editor will indent with `tab_size` spaces
`visual_empty` | `char` | the character that will be drawn to indicate end of buffer
`visual_space` | `char` | the character that will be drawn in place of spaces
`visual_tab_first` | `char` | the first character that will be drawn in place of a tab
`visual_tab_repeat` | `char` | the character that will be drawn repeatedly in place of a tab until we read a tab stop
`completion_min_len` | `integer` | min number of bytes before auto completion is triggered
`picker_max_height` | `integer` | max number of lines that are shown at a time when a picker ui is opened
`status_bar_max_height` | `integer` | max number of lines that the status bar can occupy (non zero)

## `color`
If `<value>` is present, it sets the editor theme color `<key>` to that color.
Otherwise, it returns its current color.
- usage: `color <key> [<value>]`

key |  doc
--- | ---
`background` | The color displayed behind the characters on the screen
`highlight` | The color of search highlights that appear behind search matches. Also the cursor color while in insert mode
`statusbar_active_background` | The background color for the focused client's statusbar
`statusbar_inactive_background` | The background color for the unfocused client's statusbar
`normal_cursor` | The cursor color while in normal mode
`select_cursor` | The cursor color while in normal mode and selecting text
`insert_cursor` | The cursor color while in insert mode
`inactive_cursor` | The cursor color for unfocused clients
`token_whitespace` | All highlighted `whitespace` tokens have this color
`token_text` | All highlighted `text` tokens have this color
`token_comment` | All highlighted `comment` tokens have this color
`token_keyword` | All highlighted `keyword` tokens have this color
`token_type` | All highlighted `type` tokens have this color
`token_symbol` | All highlighted `symbol` tokens have this color
`token_string` | All highlighted `string` tokens have this color
`token_literal` | All highlighted `literal` tokens have this color

## `map`
Creates a keyboard mapping for an editor mode.
`<mode>` is one of `normal`, `insert`, `command`, `readline` and `picker`.
`<from>` and `<to>` are a string of keys.
- usage: `map <mode> <from> <to>`

## `syntax`
Either begins a new syntax definition for buffer paths that match a glob `<glob>`,
or sets the pattern for tokens of kind `<token-kind>` for the previously defined syntax.
`<token-kind>` is one of `keywords`, `types`, `symbols`, `literals`, `strings`, `comments` and `texts`.
- usage: `syntax <glob>` or `syntax <token-kind> <pattern>`

Read more about [language syntax definitions](language_syntax_definitions.md).

## `copy-command`
Sets the command to be used when copying text to clipboard.
The copied text is written to stdin utf8 encoded.
This is most useful on platforms that do not have an unique way to interact with the clipboard.
If `<command>` is empty, no command is used.
- usage: `copy-command <command>`

By default, this is set per platform:
- windows: empty (uses win32 clipboard api)
- linux: `xsel --clipboard --input`
- bsd: `xclip -in`
- mac: `pbcopy`

## `paste-command`
Sets the command to be used when pasting text from clipboard.
The pasted text is read from stdout and needs to be utf8 encoded.
This is most useful on platforms that do not have an unique way to interact with the clipboard.
If `<command>` is empty, no command is used.
- usage: `paste-command <command>`

By default, this is set per platform:
- windows: empty (uses win32 clipboard api)
- linux: `xsel --clipboard --output`
- bsd: `xclip -out`
- mac: `pbpaste`

## `enqueue-keys`
Enqueue keys as if they were typed in the current client.
- usage: `enqueue-keys <keys>`

## `insert-text`
Deletes text inside all cursor ranges. Then inserts `<text>` at each cursor.
Equivalent to `enqueue-keys i<text><enter>` however more performant since the text insertion happens at once instead of char by char.
- usage: `insert-text <text>`

## `set-register`
Set the content of register `<key>` to `<value>`.
- usage: `set-register <key> <value>`

## `set-clipboard`
Sets the contents of the system clipboard to `<text>`.
- usage: `set-clipboard <text>`

## `set-env`
Set the value of the environment variable `<key>` to `<value>`
- usage: `set-env <key> <value>`

## `readline`
Enters readline mode and once a line is read, executes the commands in `<continuation>`.
It's possible to access the line input through `@register(i)` when `<continuation>` executes.
- usage: `readline <continuation>`

## `pick`
Enters picker mode and once an entry is selected, executes the commands in `<continuation>`.
It's possible to access the selected entry input through `@picker-entry()` when `<continuation>` executes.
- usage: `pick <continuation>`

## `picker-entries`
Clears and then adds all `<entries...>` to be selected with the `pick` command.
- usage: `picker-entries <entries...>`

## `picker-entries-from-lines`
Clears and then adds a picker entry for each `<command>` stdout line (with stdin closed) to be selected with the `pick` command.
- usage: `picker-entries-from-lines <command>`

## `spawn`
Spawns the external `<command>` (with stdin closed and ignoring its stdout).
- usage: `spawn <command>`

## `replace-with-output`
Pass each cursor selection as stdin to the external `<command>` and substitute each for its stdout.
- usage: `replace-with-output <command>`

## `command`
Defines a new command that can be called by its `<name>` which executes all commands in its `<source>`.
Commands which name starts with `-` won't show up in the command completion menu.
- usage: `command <name> <source>`

## `eval`
Evaluate `<commands>` as if they were typed in directly.
However it enables expansions to happen before evaluation.
- usage: `eval <commands>`

## `if`
Conditionally evaluate `<commands>` as if they were typed in directly.
However it enables expansions to happen before evaluation.
`<op>` can be one of the following:
- `==`: executes if `<left-expr>` is equal to `<right-expr>`
- `!=`: executes if `<left-expr>` is not equal to `<right-expr>`

- usage: `if <left-expr> <op> <right-expr> <commands>`

