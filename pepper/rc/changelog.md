# changelog

# 0.30.0 (preview)
- added `insert-text` command
- added `set-clipboard` command
- fix `>` (indent command) will no longer indent empty lines
- changed search string will *also* be compiled into a pattern if it contains `^` or `$`
- changed readline input and prompt are now registers `i` and `p` respectively
- removed `@readline-input()` as the input can now be accessed by `register(i)`
- added `toggle-comment` which will toggle a prefix comment for each line reached by a cursor
- added a simple indentation fixer on the `=` binding while in normal mode
- added batch file syntax
- fixed string syntax for some languages (lua, js, html, css, py)
- added ruby syntax
- fix crash when opening and editing the middle of a buffer in a single action (in a macro or through lsp for example)
- fix `` [aA]` `` was missing
- added `log` as a `open` command option which is the same as old `scratch` and now `scratch` enables buffer history by default
- added `file-backed` as a `open` command option which affects open and reopen commands
- added `output` as a `open` command option which disables all buffer prorperties (suitable for process outputs)
- added `to-lowercase` and `to-uppercase` commands
- fix buffer views would not be removed when their clients closed
- fix breakpoints locations on text edits
- changed `save-all` and `reopen-all` to only report an error at the end and always try to process all buffers
- added `BufferBreakpointId` to `BufferBreakpoint` which is a buffer scoped monotonically increasing breakpoint id
- changed `open-log` to always refreshes the log buffer's content
- changed `plugin-remedybg` to make use of the new IPC driver api which enables better integration
- changed `help` command to autocomplete on help page names and always open help pages
- fix web version not working because it would try to create a log file
- changed cursor list format from "path:1,3-1,4;2,3-2,4" to "path:1:3-1:4,2:3-2:4"

# 0.29.0
- added diagnostic logging to `spawn` command
- added diagnostic logging to `replace-with-output` command
- fix pattern bad optimization outupt when nonascii codepoints
- fix `remedybg-plugin` breakpoint sync on spawn
- changed default value of `indent_with_tabs` to false
- removed `xa` binding that used to list all breakpoints in a buffer
- added `buffer-list` command that lists all buffers in a new buffer
- added `lint-list` command that lists all lints in a buffer
- added `breakpoint-list` command that lists all lints in a buffer
- fix breakpoint was not rendering if it was on the first line of the view
- added proper error flow handling in some builtin commands that either tried to open a file or parse process command
- fixed undo on a buffer with `history-disabled` would move the cursor to the top
- fixed would not open a buffer with `saving-disabled` and invalid chars in its path
- fixed `gf` would not open file when it's relative to the root and the current buffer's path parent exists
- added support for parsing multiple cursors when using (`m_`, `gf`, etc)
- added main cursor range display in status bar when its anchor is different from its position
- added `@cursor-anchor()` and `@cursor-position()` expansions which expand with the format `line,col`
- added `@cwd()` expansion which expands into the current directory path
- removed unnecessary conditionals on ui rendering code
- added the chars `[]{}` as path delimiters for `find_path_and_ranges_at` (`gf` on normal mode)
- added cursor count to the status bar when there is more than one cursor

# 0.28.0
- added the concept of breakpoints for plugins to use
- added bindings starting with `x` that interact with breakpoints
- changed theme color name from `background` to `normal_background`
- changed theme color name from `active_line_background` to `active_background`
- added theme color `breakpoint_background`
- changed smart search patterns: if your search pattern contains a `%` character, it will perform a pattern search instead of a fixed string search (it's still possible to force a fixed string search by prefixing it with either `f/` or `F/`)
- changed `find_path_and_position_at` to also break on `"` and `'`
- added remedybg plugin (under the `plugin-remedybg` folder)
- added css syntax
- changed bracket objects to invert bracket positions if invoked with the closing bracket. that is, `a)` will now select text between `)` and `(` instead of `(` and `)`
- changed `StatusBar` to `Logger` (and `editor.status_bar` to `editor.logger`)
- changed `print` command to `log` command which accepts a `<log-kind>` parameter (use `log status <args...>` for old behavior)
- added logging to the editor which you can open it with the `open-log` command
- added `if` command that supports `==` and `!=` operations to conditionally execute other commands
- added `@platform()` that expands to the platform name the editor is running on top of.
- added the possibility for plugins to register its own expansions
- added command variable expansion completion
- better picker heuristics which favors matches that happen more to the end of the entry
- fix lsp-plugin documentation on globs

# 0.27.0
- added `set-env` command to change the editor's environment variables
- fix `@arg(*)` expanding into no arguments if it's empty
- fix `save` command alias `s` not taking arguments as it should
- changed `cd` binding (delete all cursors except the main cursor) to `CD`
- added new `cd` binding that only deletes the main cursor
- added lsp configuration examples
- fix `gf` (and `GF`) that could open a duplicate of an already opened buffer if trying to open the same path but absolute
- fix `reopen-all` would fail if there was a scratch buffer with a path that does not exist
- changed `spawn` command to use a piped stdout in order to detect when the process exits
- changed `cursor-<anchor/position>-<column/line>` expansions to be one based (instead of zero based) for easier interoperability with other softwares

## 0.26.1
- improved `find_path_and_position_at` to account for paths followed by `:`
- unix: fix not being able to spawn server if the application was installed to a folder in path

## 0.26.0
- removed escaping expansion from `{...}` string blocks
- unix now uses `posix_spawn` instead of `fork` to spawn a server for better reliability and to remove the need to use `libc::daemon` which is deprecated on macos
- fix bug on windows that prevented the server from spawning when opening files using `--` cli positional args

## 0.25.0
- new variable expansion mechanism when evaluating commands
- changed string syntax for commands
- command strings now support some escapings
- command aliases that start with `-` won't show up in auto completions
- merged `default_commands.pepper` with `default_bindings.pepper` into `default_configs.pepper`
- merged all `map-<mode>` commands into a single `map` command whose first parameter is the mode to map keys to
- merged all `syntax-<token-kind>` commands into the `syntax` command which can take the first parameter the token kind for the defined pattern
- insert processes now correctly adjust their insert positions on buffer insertions and deletions
- added `set-register` command
- changed `open` command parameters order, now buffer properties come before the `path` parameter
- removed `alias` command since it's now possible to replicate its behavior by creating a new command that calls the aliased command and use the `@arg()` expansion
- removed `find-file` and `find-command` commands as they're now implementable using other builtin commands (see `default_configs.pepper` for an example)
- removed the old 255 cursor count limit
- exiting search mode will fully restore the previous cursor state
- it's now possible to use use the search mode to expand selections
- included default config files to help pages
- fix wrong error message when parsing color values
- fix buffer would not read from file when opened with `saving-disabled`
- lsp plugin correctly handle completion responses which only fill the `changes` field
- added `pepper-` prefix to windows session named pipe paths

## 0.24.0
- handle buffer paths beginning with `./` (on `Buffer::set_path` and `Buffer::find_with_path`)
- command `$` is now `!` and what was `!` is now removed; that is, there's no longer a 'only insert from command output', just 'replace with command output' (`|` command) and if the selection is empty, it behaves as if it was the old `!`

## 0.23.3
- fix failing lsp protocol test that should only run on windows
- force redeploy on github actions

## 0.23.2
- fix URI parsing on windows

## 0.23.1
- fix crash after pc wakeup on linux (possibly on bsd and mac as well)
- fix server occasionally dropping writes to client on linux

## 0.23.0
- changed default clipboard linux interface to `xclip` instead of `xsel`
- fix crash when `lsp-references` would not load the context buffer
- handle `<c-i>` (insert at end of line) by instead mapping it to tab on unix
- fix some lsp operations not working on unix due to poor path handling

## 0.22.0
- added quit instruction to the start screen
- added '%t' to patterns to match a tab ('\t')
- fix bad handling of BSD's resize signal on kqueue

## 0.21.0
- prevent deadlocks by writing asynchronously to clients from server
- fix possible crash when listing lsp item references when there's a usage near the end of the buffer
- added instructions on how to package the web version of the editor
- added error to `lsp-stop` and `lsp-stop-all` when there is no lsp server running

## 0.20.0
- use builtin word database for completions when plugins provide no suggestions
- prevent closing all clients when the terminal from which the server was spawned is closed
- fix debugging crash when sometimes dropping a client connection

## 0.19.3
- added changelog! you can access it through `:help changelog<enter>`
- added error number to unix platform panics
- fix event loop on bsd
- fix idle events not triggering on unix
- fix buffer history undo crash when you undo after a "insert, delete then insert" single action
- fix messy multiple autocomplete on the same line
- fix crash on macos since there kqueue can't poll /dev/tty

## 0.19.2 and older
There was no official changelog before.
However, up to this point, we were implementing all features related to the editor's vision.
Then fixing bugs and stabilizing the code base.
