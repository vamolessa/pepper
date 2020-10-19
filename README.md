# pepper
Experimental code editor

# development thread
https://twitter.com/ahvamolessa/status/1276978064166182913

# keys

## normal mode
This is the main mode from where you can interact with the editor, buffers and so on.

### navigation
keys | action
--- | ---
`h`, `j`, `k`, `l` | move cursors
`w`, `b` | move cursors by word
`n`, `p` | move main cursor to next/previous search match
`N`, `P` | add cursor to the next/previous search match if inside a search range or make a new one 
`<c-n>`, `<c-p>` | go to next/previous cursor positions in history
`gg` | go to line
`gh`, `gl`, `gi` | move cursors to first, last and first non-blank columns
`gj`, `gk` | move cursors to first/last line
`gm` | move cursors to matching bracket
`gb` | fuzzy pick from all opened buffers
`f<char>`, `F<char>` | move cursors to next/previous `<char>` (inclusive)
`t<char>`, `T<char>` | move cursors to next/previous `<char>` (exclusive)
`;`, `,` | repeat last find char in forward/backward mode
`<c-d>`, `<c-u>` | move cursors half page down/up
`/` | enter search mode

binding | action
--- | ---
`s` | enter search mode

### selection
keys | action
--- | ---
`aw`, `aW` | select word object
`a(`, `a)`, `a[`, `a]`, `a{`, `a}`, `a<`, `a>`, `a|`, `a"`, `a'` | select region inside brackets (exclusive)
`Aw`, `AW` | select word object including surrounding whitespace
`A(`, `A)`, `A[`, `A]`, `A{`, `A}`, `A<`, `A>`, `A|`, `A"`, `A'` | select region inside brackets (inclusive)
`v` | toggle selection mode
`V` | expand selections to either start or end of lines depending on their orientation
`zz`, `zj`, `zk` | scroll to center main cursor or frame the main cursor on the bottom/top of screen

### cursor manipulation
keys | action
--- | ---
`xx` | add a new cursor to each selected line
`xc` | reduce all cursors to only the main cursor
`xv` | exit selection mode
`xo` | swap the anchor and position of all cursors
`xn`, `xp` | set next/previous cursor as main cursor
`x/` | reduce selections to their insersection with search ranges

binding | action
--- | ---
`xs` | reduce selections to their insersection with search ranges

### editing
keys | action
--- | ---
`d` | delete selected text
`i` | delete selected text and enter insert mode
`<`, `>` | indent/dedent selected lines
`y` | copy selected text to clipboard
`Y` | delete selected text and paste from clipboard
`u`, `U` | undo/redo

binding | action
--- | ---
`I`, `<c-i>` | move cursors to first non-blank/last column and enter insert mode
`<o>`, `<O>` | create an empty line bellow/above each cursor and enter insert mode
`J` | join one line bellow each cursor

### scripting
keys | action
--- | ---
`:` | enter script mode

## insert mode
Insert new text to the current buffer.

keys | action
--- | ---
`<esc>` | enter normal mode
`<left>`, `<down>`, `<up>`, `<right>` | move cursors
`<char>` | insert char
`<backspace>`, `<delete>` | delete char backward/forward
`<c-w>` | delete word backward
`<c-n>`, `<c-p>` | apply next/previous completion

binding | action
--- | ---
`<c-c>` | enter normal mode
`<c-h>` | delete char backward
`<c-m>` | insert line break

## script mode
Perform actions not directly related to editing such as: open/save/close buffer, change settings, execute external programs, etc.

Functions are defined inside their category object (e.g. client functions should be accessed through `client.function_name()`).
**Function parameters are annotated with expected types. `?` denotes optional paramter.
Functions without return type means they return nothing (`nil`)**

Also, parameterless functions can be called without parenthesis if they're the sole expression being evaluated.

Shortcuts are called without their category object prefix (they're a free function).

### client
function | action
--- | ---
`index() -> integer` | the index of current client (index `0` is where the server is run)
`current_buffer_view_handle(client_index: integer?) -> integer` | client's current buffer view handle or current client's
`quit()` | try quitting current client if it's not the server and there are no unsaved buffers
`quit_all()` | try quitting all clients if there are no unsaved buffers
`force_quit_all()` | quits all clients even if there are unsaved buffers

shortcut | action
--- | ---
`q` | same as `client.quit()`
`qa` | same as `client.quit_all()`
`fqa` | same as `client.force_quit_all()`

### editor
function | action
--- | ---
`version() -> string` | the editor version string formatted as `major.minor.patch`.
`print(value: any)` | prints a value to the editor's status bar

### read_line
function | action
--- | ---
`prompt(prefix: string)` | changes the prompt for the next `read_line.read()` calls


# todo
- macros
	- repeat last insert (`.`)
	- record/play custom macros
- language server protocol
- debug adapter protocol
