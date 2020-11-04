# pepper
### An opionated modal code editor for your terminal with a focus on programmer's comfort

Pepper is an experiment to craft a code editor focused on programmer's comfort.
It was born out of my frustrations with code editors throughout the years. Those editors, however, also helped
shape my vision of what the perfect code environment would be for me.

I've drawn inspiration from (in no particular order):
- Kakoune: features a novel approach to modal editing
- Vim: arguably the most popular modal code editor
- VSCode: really nice lsp integration
- Spacemacs: ergonomic and mnemonic interface
- Sublime: popularized multi-cursor editing. also, it's super snappy!
- Amp: minimalistic and lightweight editor

## handmade
In the spirit of Handmade, almost all features are coded from scratch.
These are the only external crates being used in the project (mainly because of easier crossplatform compatibility):
- `ctrlc`: prevents closing application on `ctrl-c` on all platforms
- `crossterm`: crossplatform terminal interaction
- `argh`: process complex cli args. eases rapid prototyping of new cli features
- `polling`: crossplatform socket events
- `mlua`: adds support for lua scripting. could be own new scripting language, however there's value on using a known one
- `fuzzy-matcher`: fuzzy matching for the picker ui. it could be replaced, however it's implementation does not get in the way and has minimal dependencies

## modal editing
Pepper is modal which means keypresses do different things depending on which mode you're in.
However, it's also designed to have few modes so the overhead is minimal. Most of the time, users will be in
either `normal` or `insert` mode. Regarding modes, you can think of Pepper like an editor that sits between
Kakoune and Vim. Like Kakoune, you manage selections directly in `normal` mode. Although unlike it, movement
commands only expand selections if you were selecting previously, kinda like Vim's `visual` mode. By doing so,
it does not has to have several `shift-` and `alt-` keybindings, leading to a more comfortable editing experience
at the cost of slight more key presses.

It features:
- everything is reachable through the keyboard
- modal editing
- multiple cursors
- client/server architecture (multiple windows and allows interacting with running instances from outside)
- lua scripting
- simple syntax highlighting
- text-objects
- many of the vim commands you're used to

# keys
These are the default keybindings. Users can remap them.

They may change during development (pre 1.0).

** under construction **

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

binding | expands to | action
--- | --- | ---
`s` | `/` | enter search mode

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
`cc` | splits all selection in lines
`cd` | clear all extra cursors and keep only the main cursor
`cv` | exit selection mode
`co` | swap the anchor and position of all cursors
`cj`, `ck` | add a new cursor to the line bellow/above the bottom/top cursor
`cn`, `cp` | set next/previous cursor as main cursor
`cs` | search inside selections and only keep those ranges
`cS`, `CS` | search inside selections and remove those ranges
`cf` | filter selections and keep the ones that contains the sear
`cF`, `CF` | search inside selections and remove those ranges

binding | expands to | action
--- | --- | ---
`cs` | `c/` | reduce selections to their insersection with search ranges
`<esc>`, `<c-c>` | `<esc>c0cv/<esc>` | keep only main cursor, remove selections, exit selection mode and clears search highlight

### editing
keys | action
--- | ---
`d` | delete selected text
`i` | delete selected text and enter insert mode
`<`, `>` | indent/dedent selected lines
`y` | copy selected text to clipboard
`Y` | delete selected text and paste from clipboard
`u`, `U` | undo/redo

binding | expands to | action
--- | --- | ---
`I`, `<c-i>`, | `dgii`, `dgli` | move cursors to first non-blank/last column and enter insert mode
`<o>`, `<O>` | `dgli<enter>`, `dgii<enter><up>` | create an empty line bellow/above each cursor and enter insert mode
`J` | `djgivkgli<space><esc>` | join one line bellow each cursor

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

binding | expands to | action
--- | --- | ---
`<c-c>` | `<esc>` | enter normal mode
`<c-h>` | `<backspace>` | delete char backward
`<c-m>` | `<enter>` | insert line break

## script mode
Perform actions not directly related to editing such as: open/save/close buffer, change settings, execute external programs, etc.

**Function parameters are annotated with expected types. `?` denotes optional paramter.
Functions without return type means they return nothing (`nil`)**

Also, parameterless functions can be called without parenthesis if they're the sole expression being evaluated.

### client
function | action
--- | ---
`client.index() -> integer` | the index of current client (index `0` is where the server is run)
`client.current_buffer_view_handle(client_index: integer?) -> integer` | client's current buffer view handle or current client's
`client.quit()` | try quitting current client if it's not the server and there are no unsaved buffers
`client.quit_all()` | try quitting all clients if there are no unsaved buffers
`client.force_quit_all()` | quits all clients even if there are unsaved buffers

shortcut | action
--- | ---
`q()` | same as `client.quit()`
`qa()` | same as `client.quit_all()`
`fqa()` | same as `client.force_quit_all()`

### editor
function | action
--- | ---
`editor.version() -> string` | the editor version string formatted as `major.minor.patch`.
`editor.os() -> string` | the os name the editor was compiled on. possible values: https://doc.rust-lang.org/std/env/consts/constant.OS.html.
`editor.print(value: any)` | prints a value to the editor's status bar

### buffer
function | action
--- | ---
`buffer.all_handles` | 
`buffer.line_count` | 
`buffer.line_at` | 
`buffer.path` | 
`buffer.extension` | 
`buffer.has_extension` | 
`buffer.needs_save` | 
`buffer.set_search` | 
`buffer.open` | 
`buffer.close` | 
`buffer.force_close` | 
`buffer.force_close_all` | 
`buffer.save` | 
`buffer.save_all` | 
`buffer.commit_edits` | 
`buffer.on_open` | 

### buffer_view
function | action
--- | ---

### cursors
function | action
--- | ---

### read_line
function | action
--- | ---
`read_line.prompt(prefix: string)` | changes the prompt for the next `read_line.read()` calls
`read_line.read(callback: function(input: string?))` | begins a line read. If submitted, the callback is called with the line written. However, if cancelled, it is called with `nil`. 

### picker
function | action
--- | ---
`picker.prompt(prefix: string)` | changes the prompt for the next `picker.pick()` calls
`picker.reset()` | reset all entries previously set
`picker.entry(name: string, description: string?)` | add a new entry to then be picked by `picker.pick()`
`picker.pick(callback: function(name: string))` | begins picking added entries. If submitted, the callback is called with the name of the picked entry. However, if cancelled, it is called with `nil`.

### process
function | action
--- | ---
`process.pipe(exe: string, args: [string]?, input: string?) -> string` | runs `exe` process with `args` and optionally with `input` as stdin. Once the process finishes, its stdout is returned.
`process.spawn(exe: string, args: [string]?, input: string?)` | runs `exe` process with `args` and optionally with `input` as stdin. This function does not block.

### keymap
function | action
--- | ---

### syntax
function | action
--- | ---


# development thread
https://twitter.com/ahvamolessa/status/1276978064166182913


# big features todo
- language server protocol (in progress)
- debug adapter protocol
