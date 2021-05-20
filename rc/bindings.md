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
| `n`, `p` | move main cursor to next/previous search match |
| `N`, `P` | add cursor to the next/previous search match if inside a search range or make a new one  |
| `<c-n>`, `<c-p>` | go to next/previous cursor position in history |
| `gg` | go to line |
| `gh`, `gl`, `gi` | move cursors to first/last/first-non-blank columns |
| `gk`, `gj` | move cursors to first/last line |
| `gm` | move cursors to matching bracket |
| `go` | fuzzy pick from all loaded buffers |
| `gb` | open previous buffer (if any) |
| `gB`, `GB` | open the buffer that is open in the previously focused client, then that client opens its previous buffer |
| `gf` | if the filepath under the cursor exists, open it as a buffer |
| `]]<char>`, `[[<char>` | move cursors to next/previous `<char>` (inclusive) |
| `][<char>`, `[]<char>` | move cursors to next/previous `<char>` (exclusive) |
| `}`, `{` | repeat last find char in forward/backward mode |
| `<c-d>`, `<c-u>` | move cursors half page down/up |
| `<c-j>`, `<c-k>` | move cursors to next/previous blank line |
| `s` | enter search mode |
| `zz`, `zj`, `zk` | scroll to center main cursor or frame the main cursor on the bottom/top of screen |
| `q<char>` | begin recording macro to register `<char>` |
| `Q<char>` | executes keys recorded in register `<char>` |
| `rn`, `rp` | move to next/previous diagnostic (requires a running lsp server) |

**NOTE**: the register `a` always contains the last selection+edit keys.

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
| `cd` | clear all extra cursors and keep only the main cursor |
| `cl` | splits all selection in lines |
| `cj`, `ck` | add a new cursor to the line bellow/above the bottom/top cursor |
| `cn`, `cp` | set next/previous cursor as main cursor |
| `cs` | search inside selections and only keep those ranges |
| `cS`, `CS` | search inside selections and remove those ranges |
| `cf` | filter selections and keep the ones that contains the search |
| `cF`, `CF` | search inside selections and remove those ranges |

| binding | expands to | action |
| --- | --- | --- |
| `<esc>`, `<c-c>` | `cdcVs<esc>` | keep only main cursor, remove selections, exit selection mode and clears search highlight |
| `.` | `Qa` | executes auto recorded macro |
| `K` | `: lsp-hover<enter>` | display hover information (requires a running lsp server) |
| `gd` | `: lsp-definition<enter>` | jumps to where the symbol under the cursor is defined (requires a running lsp server) |
| `gr` | `: lsp-references -context=2<enter>` | lists all references of the symbol under the cursor with 2 lines of context (requires a running lsp server) |
| `gs` | `: lsp-document-symbols<enter>` | lists all symbols in the buffer (requires a running lsp server) |
| `rr` | `: lsp-rename<enter>` | rename the symbol under the cursor (requires a running lsp server) |
| `ra` | `: lsp-code-action<enter>` | suggests possible refactors for the region under the cursor (requires a running lsp server) |
| `rf` | `: lsp-format<enter>` | auto-format the buffer's content (requires a running lsp server) |

### editing

| keys | action |
| --- | --- |
| `d` | delete selected text |
| `i` | delete selected text and enter insert mode |
| `<`, `>` | indent/dedent selected lines |
| `y` | copy selected text to clipboard |
| `Y` | delete selected text and paste from clipboard |
| `<c-y><lowercase-char>` | copy selected text to register `<char>` |
| `<c-y><uppercase-char>` | delete selected text and paste the contents of register `<char>` |
| `u`, `U` | undo/redo |

| binding | expands to | action |
| --- | --- | --- |
| `I`, `<c-i>`, | `dgii`, `dgli` | move cursors to first non-blank/last column and enter insert mode |
| `ci` | `cvcCglccgii` | delete all lines touching a selection and enter insert mode |
| `o`, `O` | `dgli<enter>`, `dgii<enter><up>` | create an empty line bellow/above each cursor and enter insert mode |
| `J` | `djgivkgli<space><esc>` | join one line bellow each cursor |

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
