# pepper
Experimental code editor

# development thread
https://twitter.com/ahvamolessa/status/1276978064166182913

# keys
## normal mode
### navigation
keys | action
--- | ---
`h` | move left
`j` | move down
`k` | move up
`l` | move right
`w` | move word right
`b` | move word left
`n` | move main cursor to next search match
`p` | move main cursor to previous search match
`N` | add cursor to the next search match if inside a search range or make a new one 
`P` | add cursor to the previous search match if inside a search range or make a new one 
`<c-n>` | go to next cursor positions in history
`<c-p>` | go to previous cursor positions in history

### selection
keys | action
--- | ---
`aw` `aW` | select word object
`a(` `a)` `a[` `a]` `a{` `a}` `a<` `a>` `a|` `a"` `a'` | select region inside brackets (exclusive)
`Aw` `AW` | select word object including surrounding whitespace
`A(` `A)` `A[` `A]` `A{` `A}` `A<` `A>` `A|` `A"` `A'` | select region inside brackets (inclusive)

## insert mode

# todo
- macros
	- repeat last insert (`.`)
	- record/play custom macros
- language server protocol
- debug adapter protocol
