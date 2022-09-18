## normal mode
This is the main mode from where you can interact with the editor.
It's probably where you'll be most of the time.
From here you can enter any other mode and it's where other modes normally get back to.
It's also from where you do most of code navigation and seleciton manipulation.

### navigation

| keys | action |
| --- | --- |
| `h`, `j`, `k`, `l` | move cursors |
| `w`, `b` | move cursors forward/back by word |
| `e` | move cursors forward to the end of the word |
| `n`, `p` | move main cursor to next/previous search match |
| `N`, `P` | add cursor to the next/previous search match if inside a search range or make a new one  |
| `<c-n>`, `<c-p>` | go to next/previous cursor position in history |
| `gg` | go to line |
| `gh`, `gl`, `gi` | move cursors to first/last/first-non-blank columns |
| `gk`, `gj` | move cursors to first/last line |
| `gm` | move cursors to matching bracket |
| `go` | fuzzy pick an opened buffer |
| `gb` | open previous buffer (if any) |
| `gB`, `GB` | open the buffer that is open in the previously focused client, then that client opens its previous buffer |
| `gf` | if the filepath under the cursor exists, open it as a buffer |
| `gF`, `GF` | if the filepath under the cursor exists, open it as a buffer, then close the current buffer |
| `xx` | toggle breakpoints on all lines covered by cursors |
| `xX`, `XX` | remove breakpoints on all lines covered by cursors |
| `xB`, `XB` | remove all breakpoints on current buffer |
| `xA`, `XA` | remove all breakpoints on all buffers |
| `]]<char>`, `[[<char>` | move cursors to next/previous `<char>` (inclusive) |
| `][<char>`, `[]<char>` | move cursors to next/previous `<char>` (exclusive) |
| `}`, `{` | repeat last find char in forward/backward mode |
| `<c-d>`, `<c-u>` | move cursors half page down/up |
| `<c-j>`, `<c-k>` | move cursors to next/previous blank line |
| `s` | enter search mode |
| `zz`, `zj`, `zk` | scroll to center main cursor or frame the main cursor on the bottom/top of screen |
| `q<char>` | begin recording macro to register `<char>` |
| `Q<char>` | executes keys recorded in register `<char>` |
| `m<char>` | save current buffer and main cursor position as a marker on register `<char>` |
| `M<char>` | go to marker on register `<char>` (if it's a valid marker) |
| `rn`, `rp` | move to next/previous lint (provided by a plugin) |

**NOTE**: the register `a` always contains the last selection+edit keys.

| binding | expands to | action |
| --- | --- | --- |
| `<space>o` | `:<space>-find-file<enter>` | fuzzy pick a file |
| `<space>f` | `:<space>-find-pattern<enter>` | workspace wide search |

### text-object

| keys | action |
| --- | --- |
| `aw`, `aW` | select word object |
| `a(`, `a)`, `a[`, `a]`, `a{`, `a}`, `a<`, `a>` | select region inside brackets (exclusive) |
| <code>a&#124;</code>, `a"`, `a'`, `` a` `` | select region delimited by a pair of these brackets on the same line (exclusive) |
| `Aw`, `AW` | select word object including surrounding whitespace |
| `A(`, `A)`, `A[`, `A]`, `A{`, `A}`, `A<`, `A>` | select region inside brackets (inclusive) |
| <code>A&#124;</code>, `A"`, `A'`, `` A` `` | select region delimited by a pair of these brackets on the same line (inclusive) |

### selection

| keys | action |
| --- | --- |
| `v` | toggle selection mode |
| `V` | expand selections to either start or end of lines depending on their orientation |
| `cv` | force enter selection mode |
| `cV`, `CV` | force exit selection mode |

### cursor manipulation

| keys | action |
| --- | --- |
| `cc` | swap the anchor and position of all cursors |
| `cC`, `CC` | orientate all cursors such that their anchors come before their positions |
| `cd` | remove main cursor if there's more than one cursor |
| `cD`, `CD` | clear all extra cursors and keep only the main cursor |
| `cl` | splits all selection in lines |
| `cj`, `ck` | add a new cursor to the line bellow/above the bottom/top cursor |
| `cn`, `cp` | set next/previous cursor as main cursor |
| `cs` | search inside selections and only keep those ranges |
| `cS`, `CS` | search inside selections and remove those ranges |
| `cf` | filter selections and keep the ones that contain the search |
| `cF`, `CF` | filter selections and keep the ones that do not contain the search |

| binding | expands to | action |
| --- | --- | --- |
| `<esc>`, `<c-c>` | `cdcVs<esc>` | keep only main cursor, remove selections, exit selection mode and clears search highlight |
| `.` | `Qa` | executes auto recorded macro |

### editing

| keys | action |
| --- | --- |
| `d` | delete selected text |
| `i` | delete selected text and enter insert mode |
| `<`, `>` | indent/dedent selected lines |
| `=` | fix indent on selected lines |
| `y` | copy selected text to clipboard |
| `Y` | delete selected text and paste from clipboard |
| `<c-y><lowercase-char>` | copy selected text to register `<char>` |
| `<c-y><uppercase-char>` | delete selected text and paste the contents of register `<char>` |
| `u`, `U` | undo/redo |

| binding | expands to | action |
| --- | --- | --- |
| `I`, `<c-i>` | `dgii`, `dgli` | move cursors to first non-blank/last column and enter insert mode |
| `ci` | `cvcCglccgii` | delete all lines touching a selection and enter insert mode |
| `o`, `O` | `dgli<enter>`, `dgii<enter><up>` | create an empty line bellow/above each cursor and enter insert mode |
| `J` | `djgivkgli<space><esc>` | join one line bellow each cursor |
| `!` | `:<space>-spawn<enter>` | execute a command line (with closed stdin and ignoring its output) |
| <code>&#124;</code> | `:<space>-replace-with-output<enter>` | pass each selection as stdin to a command line and substitute each for its stdout |

## insert mode
Insert new text to the current buffer.

| keys | action |
| --- | --- |
| `<esc>`, `<c-c>` | enter normal mode |
| `<left>`, `<down>`, `<up>`, `<right>` | move cursors |
| `<char>` | insert `<char>` to the left of every cursor |
| `<enter>`, `<c-m>` | insert line break to the left of every cursor |
| `<backspace>`, `<c-h>` | delete char backward |
| `<delete>` | delete char forward |
| `<c-w>` | delete word backward |
| `<c-n>`, `<c-p>` | apply next/previous completion |

## command mode
Perform actions not directly related to editing such as: open/save/close buffer, change settings, execute external programs, etc.
In order to enter command mode, type `:` while in normal mode.

When the input is empty, you can navigate through history with `<c-n>` and `<c-p>`.

**NOTE**: if a command starts with a space, it will not be recorded to the command history.

Also, `<c-n>` and `<c-p>` will choose from the autocomplete entries.

See the [command reference](command_reference.md).
