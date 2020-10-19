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

### editing
keys | action
--- | ---
`d` | delete selected text
`i` | delete selected text and enter insert mode
`<`, `>` | indent/dedent selected lines
`y` | copy selected text to clipboard
`Y` | delete selected text and paste from clipboard
`u`, `U` | undo/redo

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

## script mode
Perform actions not directly related to editing such as: open/save/close buffer, change settings, execute external programs, etc.

# todo
- macros
	- repeat last insert (`.`)
	- record/play custom macros
- language server protocol
- debug adapter protocol
