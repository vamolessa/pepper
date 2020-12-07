# modes

### !!! UNDER CONSTRUCTION! These may change at any time !!!

## normal mode
This is the main mode from where you can interact with the editor.
It's probably where you'll be most of the time.
From here you can enter any other mode and it's where other modes normally get back to.
It's also from where you do most of code navigation and seleciton manipulation.

### navigation

| keys | action |
| --- | --- |
| `h`, `j`, `k`, `l` | move cursors |
| `w`, `b` | move cursors by word |
| `n`, `p` | move main cursor to next/previous search match |
| `N`, `P` | add cursor to the next/previous search match if inside a search range or make a new one  |
| `<c-n>`, `<c-p>` | go to next/previous cursor positions in history |
| `gg` | go to line |
| `gh`, `gl`, `gi` | move cursors to first, last and first non-blank columns |
| `gj`, `gk` | move cursors to first/last line |
| `gm` | move cursors to matching bracket |
| `gb` | fuzzy pick from all opened buffers |
| `f<char>`, `F<char>` | move cursors to next/previous `<char>` (inclusive) |
| `t<char>`, `T<char>` | move cursors to next/previous `<char>` (exclusive) |
| `;`, `,` | repeat last find char in forward/backward mode |
| `<c-d>`, `<c-u>` | move cursors half page down/up |
| `s` | enter search mode |
| `zz`, `zj`, `zk` | scroll to center main cursor or frame the main cursor on the bottom/top of screen |

### text-object

| keys | action |
| --- | --- |
| `aw`, `aW` | select word object |
| `a(`, `a)`, `a[`, `a]`, `a{`, `a}`, `a<`, `a>` | select region inside brackets (exclusive) |
| `a<pipe>`, `a"`, `a'` | select region delimited by a pair of these brackets on the same line (exclusive) |
| `Aw`, `AW` | select word object including surrounding whitespace |
| `A(`, `A)`, `A[`, `A]`, `A{`, `A}`, `A<`, `A>` | select region inside brackets (inclusive) |
| `A<pipe>`, `A"`, `A'` | select region delimited by a pair of these brackets on the same line (inclusive) |

** `<pipe>` is `|`. I had to write like this because of markdown table formatting.

### selection

| keys | action |
| --- | --- |
| `v` | toggle selection mode |
| `V` | expand selections to either start or end of lines depending on their orientation |

### cursor manipulation

| keys | action |
| --- | --- |
| `cc` | splits all selection in lines |
| `cd` | clear all extra cursors and keep only the main cursor |
| `cv` | exit selection mode |
| `co` | swap the anchor and position of all cursors |
| `cj`, `ck` | add a new cursor to the line bellow/above the bottom/top cursor |
| `cn`, `cp` | set next/previous cursor as main cursor |
| `cs` | search inside selections and only keep those ranges |
| `cS`, `CS` | search inside selections and remove those ranges |
| `cf` | filter selections and keep the ones that contains the search |
| `cF`, `CF` | search inside selections and remove those ranges |

| binding | expands to | action |
| --- | --- | --- |
| `<esc>`, `<c-c>` | `<esc>c0cv/<esc>` | keep only main cursor, remove selections, exit selection mode and clears search highlight |

### editing

| keys | action |
| --- | --- |
| `d` | delete selected text |
| `i` | delete selected text and enter insert mode |
| `<`, `>` | indent/dedent selected lines |
| `y` | copy selected text to clipboard |
| `Y` | delete selected text and paste from clipboard |
| `u`, `U` | undo/redo |

** for now copy/paste (`y` and `Y`) is disabled because I could not build it on linux because of a missing X11 dependency there.
My solution, for now, is to call the platform's own `clip` program. On windows, I use my own [clipboard interface](https://github.com/matheuslessarodrigues/copycat).

| binding | expands to | action |
| --- | --- | --- |
| `I`, `<c-i>`, | `dgii`, `dgli` | move cursors to first non-blank/last column and enter insert mode |
| `<o>`, `<O>` | `dgli<enter>`, `dgii<enter><up>` | create an empty line bellow/above each cursor and enter insert mode |
| `J` | `djgivkgli<space><esc>` | join one line bellow each cursor |

### scripting

| keys | action |
| --- | --- |
| `:` | enter script mode |

## insert mode
Insert new text to the current buffer.

| keys | action |
| --- | --- |
| `<esc>` | enter normal mode |
| `<left>`, `<down>`, `<up>`, `<right>` | move cursors |
| `<char>` | insert char |
| `<backspace>`, `<delete>` | delete char backward/forward |
| `<c-w>` | delete word backward |
| `<c-n>`, `<c-p>` | apply next/previous completion |

| binding | expands to | action |
| --- | --- | --- |
| `<c-c>` | `<esc>` | enter normal mode |
| `<c-h>` | `<backspace>` | delete char backward |
| `<c-m>` | `<enter>` | insert line break |

## script mode
Perform actions not directly related to editing such as: open/save/close buffer, change settings, execute external programs, etc.

See the [scripting api](scripting).
