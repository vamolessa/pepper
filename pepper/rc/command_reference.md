These are the builtin commands that can be used to interact with the editor.
Their main purpose is to provide features that complement text editing.
Commands is also how you can configure your editor on config files.

Also, when passing arguments that contain spaces, you can wrap them between `"`, `'` or `{{` and `}}`
(similar to lua strings, you can put any number of `=` between the `{` or `}` as long as they match).

# builtin commands

## `help`
Searches the help pages for `<keyword>`.
If `<keyword>` is not present, opens the main help page.
- default alias: `h`
- usage: `help [<keyword>]`

## `print`
Prints earch `<argument>` to status bar. Each argument is separated by a new line.
- usage: `print <arguments...>`

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
That is, calling `open my-buffer history-enabled scratch` will actually open `my-buffer` with undo history disabled!

- usage: `open <path> [<properties...>]`
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

## `map-normal`, `map-insert`, `map-command`, `map-readline`, `map-picker`
Creates a keyboard mapping for an editor mode.
`<from>` and `<to>` are a string of keys.
- usage: `map-<mode> <from> <to>`

## `alias`
Create a alias with name `<name>` for the command `<command>`.
Note that `<command>` can also contain arguments which will expand when calling the alias.
- usage: `alias <name> <command>`

## `syntax`
Begins a new syntax definition for buffer paths that match a glob `<glob>`.
In order to specify each syntax pattern, the other `syntax-<token-kind>` commands are used.
- usage: `syntax <glob>`

Read more about [language syntax definitions](language_syntax_definitions.md).

## `syntax-keywords`
Sets the pattern for tokens of kind 'keyword' for the previously defined syntax (see the `syntax` command).
- usage: `syntax-keywords <pattern>`

## `syntax-types`
Sets the pattern for tokens of kind 'type' for the previously defined syntax (see the `syntax` command).
- usage: `syntax-types <pattern>`

## `syntax-symbols`
Sets the pattern for tokens of kind 'symbol' for the previously defined syntax (see the `syntax` command).
- usage: `syntax-symbols <pattern>`

## `syntax-literals`
Sets the pattern for tokens of kind 'literal' for the previously defined syntax (see the `syntax` command).
- usage: `syntax-literals <pattern>`

## `syntax-strings`
Sets the pattern for tokens of kind 'string' for the previously defined syntax (see the `syntax` command).
- usage: `syntax-strings <pattern>`

## `syntax-comments`
Sets the pattern for tokens of kind 'comment' for the previously defined syntax (see the `syntax` command).
- usage: `syntax-comments <pattern>`

## `syntax-texts`
Sets the pattern for tokens of kind 'text' for the previously defined syntax (see the `syntax` command).
- usage: `syntax-texts <pattern>`

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

## `on-platforms`
Given a `<platforms...>` argument list, execute `<commands>` if we're on any of such platforms.
Note that `<commands>` are interpreted the same way a config file is interpreted
(that is, line by line but also supporting multiline commands).
- usage: `on-platforms <platforms...> <commands>`


## `find-file`
Executes external command `<command>` and fills the picker menu from each line of its stdout.
When an entry is selected, it's opened as a buffer path.
Also, it's possible to customize the `<prompt>` that is shown on the picker ui.
- usage: `find-file <command> [<prompt>]`

## `find-pattern`
Shows a readline ui that queries for the search pattern.
When it's submitted, the external command `<command>` whose stdout will be inserted into a buffer named `<command>.refs`.
Note that any `{}` in `<command>` will be substituted by the search pattern.
Also, it's possible to customize the `<prompt>` that is shown on the readline ui.
- usage: `find-pattern <command> [<prompt>]`

