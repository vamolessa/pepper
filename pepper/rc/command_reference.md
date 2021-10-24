These are the builtin commands that can be used to interact with the editor.
Their main purpose is to provide features that complement text editing.
Commands is also how you can configure your editor on config files.

Also, when passing text values as arguments, you can wrap them between `"`, `'` or `[[` and `]]` (like lua strings).

# builtin commands

## `help`
Searches the help pages for `<keyword>`.
If `<keyword>` is not present, opens the main help page.
- alias: `h`
- usage: `help [<keyword>]`

## `try`
Try executing commands without propagating errors.
Then optionally executes commands if there was an error.
- usage: `try { <commands...> } [catch { <commands...> }]`

## `macro`
Defines a new macro command.
A `<param-register>` should be the register name that will contain the arg value.
- usage: `macro [<flags>] <name> <param-registers...> { <commands> }`
- flags:
  - `-hidden` : whether this command is shown in completions or not

## `request`
Register a request command for this client.
The client needs to implement the editor protocol.
Because of that, it only makes sense to use this if it's called from a custom client.
- usage: `request [<flags>] <name>`
- flags:
  - `-hidden` : whether this command is shown in completions or not

## `copy-command`
Sets the command to be used when copying text to clipboard.
The copied text is written to stdin utf8 encoded.
This is most useful on platforms that do not have an unique way to interact with the clipboard.
If `<command>` is empty, no command is used.
- usage: `copy-command <command>`

## `paste-command`
Sets the command to be used when pasting text from clipboard.
The pasted text is read from stdout and needs to be utf8 encoded.
This is most useful on platforms that do not have an unique way to interact with the clipboard.
If `<command>` is empty, no command is used.
- usage: `paste-command <command>`

## `spawn`
Spawns a new process and then optionally executes commands on its output.
Those commands will be executed on every splitted output if `-split-on-byte` is set
or on its etirety when the process exits otherwise.
Output can be accessed from the `%z` register in `<commands-on-output>`
- usage: `spawn [<flags>] <spawn-command> [<commands-on-output...>]`
- flags:
  - `-input=<text>` : sends `<text>` to the stdin
  - `-env=<vars>` : sets environment variables in the form `VAR=<value> VAR=<value>...`
  - `-split-on-byte=<number>` : splits process output at every <number> byte

## `replace-with`
If either `-from` or `-to` are present, then the text inside that range will be deleted, otherwise
each cursor selection will be used as a delete range. Then, it inserts `<text>` at every delete range,
effectivelly replacing its previous contents.
- usage: `replace-with [<flags>] <text>`
- flags:
  - `-buffer=<buffer-id>` : if present, buffer with id `<buffer-id>` is used instead
  - `-from=<position>` : if present, replace range will start at `<position>`
  - `-to=<position>` : if present, replace range will end at `<position>`

## `replace-with-output`
Replace each cursor selection with command output.
- usage: `replace-with-output [<flags>] <command>`
- flags:
  - `-pipe` : also pipes selected text to command's input
  - `-env=<vars>` : sets environment variables in the form VAR=<value> VAR=<value>...
  - `-split-on-byte=<number>` : splits output at every <number> byte

## `execute-keys`
Executes keys as if they were inputted manually.
- usage: `execute-keys <keys>`
- flags:
  - `-client=<client-id>` : send keys on behalf of client `<client-id>`

## `read-line`
Prompts for a line read and then executes commands.
The line read can be accessed from the `%z` register in `<commands>`.
- usage: `read-line [<flags>] <commands...>`
- flags:
  - `-prompt=<prompt-text>` : the prompt text that shows just before user input (default: `read-line:`)

## `pick`
Opens up a menu from where an option can be picked and then executes commands.
Options can be added with the `add-picker-option` command.
The picked entry can be accessed from the `%z` register in `<commands>`.
- usage: `pick [<flags>] <commands>`
- flags:
  - `-prompt=<prompt-text>` : the prompt text that shows just before user input (default: `pick:`)

## `add-picker-option`
Adds a new picker option that will then be shown in the next call to the `pick` command.
- usage: `add-picker-option <name>`

## `quit`
Quits this client.
With '!' will discard any unsaved changes.
- usage `quit[!]`
- alias: `q`

## `quit-all`
Quits all clients.
With '!' will discard any unsaved changes.
- usage: `quit-all[!]`
- alias: `qa`

## `print`
Prints `<values>` to the status bar.
- usage: `print [<flags>] <values...>`
- flags:
  - `-error` : will print as an error
  - `-dbg` : will also print to the stderr

## `source`
Sources file at `<path>` and executes its contents as commands.
With '!' will do nothing if file does not exist instead of raising an error.
- usage: `source[!] <path>`

## `open`
Opens a buffer up for editting.
If file `<path>` exists, it will be loaded into the buffer's content.
- usage: `open [<flags>] <path>`
- alias: `o`
- flags:
  - `-line=<number>` : set cursor at line
  - `-column=<number` : set cursor at column
  - `-no-history` : disables undo/redo
  - `-no-save` : disables saving
  - `-no-word-database` : words in this buffer will not contribute to the word database
  - `-auto-close` : automatically closes buffer when no other client has it in focus

## `save`
Saves buffer to file.
If `<path>` is present, it will use that path so save the buffer's content,
making it the new buffer's associated filepath.
- usage: `save [<flags>] [<path>]`
- alias: `s`
- flags:
  - `-buffer=<buffer-id>` : if present, buffer with id `<buffer-id>` is used instead

## `save-all`
Saves all buffers to file.
- usage: `save-all`
- alias: `sa`

## `reload`
Reloads buffer from file.
With '!' will discard any unsaved changes.
- usage: `reload[!] [<flags>]`
- alias: `r`
- flags:
  - `-buffer=<buffer-id>` : if present, buffer with id `<buffer-id>` is used instead

## `reload-all`
Reload all buffers from file.
With '!' will discard any unsaved changes
- usage: `reload-all[!]`
- alias: `ra`

## `close`
Closes current buffer and opens previous viewed buffer if any.
With '!' will discard any unsaved changes.
- usage: `close[!] [<flags>]`
- alias: `c`
- flags:
  - `-buffer=<buffer-id>` : if present, buffer with id `<buffer-id>` is used instead
  - `-no-previous-buffer` : does not try to open previous buffer

## `close-all`
Closes all buffers.
With '!' will discard any unsaved changes.
- usage: `close-all[!]`
- alias: `ca`

## `config`
If `<value>` is present, it sets the editor config `<key>` to its value.
Otherwise, it returns its current value.
- usage: `config <key> [<value>]`

key | type | doc
--- | --- | ---
`tab_size` | `integer` | size of a tab relative to space
`indent_with_tabs` | `bool` | if false, the editor will indent with `tab_size` spaces
`visual_empty` | `char` | the character that will be drawn to indicate end of buffer
`visual_space` | `char` | the character that will be drawn in place of spaces
`visual_tab_first` | `char` | the first character that will be drawn in place of a tab
`visual_tab_repeat` | `char` | the character that will be drawn repeatedly in place of a tab until we read a tab stop
`completion_min_len` | `integer` | min number of bytes before auto completion is triggered
`picker_max_height` | `integer` | max number of lines that are shown at a time when a picker ui is opened

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

## `syntax`
Creates a syntax definition from patterns for files that match a glob.
Every line in `<definition>` should be of the form: `<token-kind> = <pattern>` where:
- `<token-kind>` is one of: `keywords`, `types`, `symbols`, `literals`, `strings`, `comments` or `texts`;
- `<pattern>` is the pattern that matches that kind of token;
- usage: `syntax <glob> { <definition> }`

Read more about [language syntax definitions](language_syntax_definitions.md).

## `map`
Creates a keyboard mapping for an editor mode.
- usage: `map [<flags>] <from> <to>`
- flags:
  - `-normal` : set mapping for normal mode
  - `-insert` : set mapping for insert mode
  - `-read-line` : set mapping for read-line mode
  - `-picker` : set mapping for picker mode
  - `-command` : set mapping for command mode

## `text-len`
Returns text length in bytes.
- usage: `text-len <text>`

## `text-join`
Returns all `<args>` joined.
- usage: `text-join <args...>`

## `client-id`
Returns the current client's id.
- usage: `client-id`

## `buffer-id`
Returns the current buffer's id.
- usage: `buffer-id`

## `buffer-path`
Returns the current buffer's associated filepath.
- usage: `buffer-path [<flags>]`
- flags:
  - `-buffer=<buffer-id>` : if present, buffer with id `<buffer-id>` is used instead

## `buffer-line-count`
Returns how many lines a buffer has. It's always at least one.
- usage: `buffer-line-count [<flags>]`
- flags:
  - `-buffer=<buffer-id>` : if present, buffer with id `<buffer-id>` is used instead

## `buffer-text`
Returns the buffer text optionally delimited by `-from` and `-to`.
- usage: `buffer-text [<flags>]`
- flags:
  - `-buffer=<buffer-id>` : if present, buffer with id `<buffer-id>` is used instead
  - `-from=<position>` : if present, text range will start at `<position>`
  - `-to=<position>` : if present, text range will end at `<position>`

## `lsp`
Automatically starts a lsp server when a buffer matching a glob is opened.
The lsp command only runs if the server is not already running.
- usage: `lsp [<flags>] <glob> <lsp-command>`
- flags:
  - `-log=<buffer-name>` : redirects the lsp server output to this buffer
  - `-env=<vars>` : sets environment variables in the form `VAR=<value> VAR=<value>...`

## `lsp-start`
Manually starts a lsp server.
- usage: `lsp-start [<flags>] <lsp-command>`
- flags:
  - `-root=<path>` : the root path from where the lsp server will execute
  - `-log=<buffer-name>` : redirects the lsp server output to this buffer
  - `-env=<vars>` : sets environment variables in the form `VAR=<value> VAR=<value>...`

## `lsp-stop`
Stops the lsp server associated with the current buffer.
- usage: `lsp-stop`

## `lsp-stop-all`
Stops all lsp servers.
usage: `lsp-stop-all`

## `lsp-hover`
Displays lsp hover information for the current buffer's main cursor position.
- usage: `lsp-hover`

## `lsp-definition`
Jumps to the location of the definition of the item under the main cursor found by the lsp server.
- usage: `lsp-definition`

## `lsp-references`
Opens up a buffer with all references of the item under the main cursor found by the lsp server.
- usage: `lsp-references [<flags>]`
- flags:
  - `-context=<number>` : how many lines of context to show. 0 means no context is shown
  - `-auto-close` : automatically closes buffer when no other client has it in focus

## `lsp-rename`
Renames the item under the main cursor through the lsp server.
- usage: `lsp-rename`

## `lsp-code-action`
Lists and then performs a code action based on the main cursor context.
- usage: `lsp-code-action`

## `lsp-document-symbols`
Pick and jump to a symbol in the current buffer listed by the lsp server.
- usage: `lsp-document-symbols`

## `lsp-workspace-symbols`
Opens up a buffer with all symbols in the workspace found by the lsp server
optionally filtered by a query.
- usage: `lsp-workspace-symbols [<query>]`

## `lsp-format`
Format a buffer using the lsp server.
- usage: `lsp-format`
