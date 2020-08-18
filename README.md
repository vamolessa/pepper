# pepper
Experimental code editor

# development thread
https://twitter.com/ahvamolessa/status/1276978064166182913

# todo
- ~~undo/redo~~
	- ~~store/apply edit diffs~~
	- limit history size??
- ~~modes~~
	- ~~basic implementation~~
	- ~~key chords actions~~
- ~~selection~~
	- ~~swap position and anchor~~
	- ~~selection merging~~
- ~~multiple cursors~~
	- ~~merge cursors~~
- ~~long lines~~
- ~~search~~
	- ~~highlight search matches~~
	- ~~navigate between search matches~~
- ~~operations~~
	- ~~delete~~
	- ~~copy~~
	- ~~paste~~
- ~~client/server model~~
	- ~~dumb client sends Keys receives EditorOperations~~
	- ~~track client that last send message (focused)~~
	- ~~show error on focused client~~
	- ~~reuse allocation when deserializing EditorOperation::Content~~
- ~~custom bindings~~
	- ~~custom bindings expand to builtin bindings~~
	- ~~custom bindings take precedence~~
	- ~~define custom bindings in config file~~
- command mode
	- ~~basic command mode~~
	- ~~default commands~~
	- define custom commands in config file?? (or just aliases??)
- ~~syntax highlighting~~
	- ~~simple pattern matching~~
	- ~~define language syntaxes~~
	- ~~calculate highlight ranges when code changes~~
	- ~~recalculate only changed portions of buffer~~
	- ~~show whitespace with correct colors~~
- ~~utf8~~
- ~~file operations~~
	- ~~edit (command to open/create file?)~~
	- ~~save~~
	- ~~reuse buffer if already open~~
	- ~~remove all buffer views (and viewport handles) when closing a buffer~~
- config file
	- load config file at startup
	- reload config file when changed??
- code navigation
	- home/end
	- find char
- text objects
	- word
	- braces
- status bar
	- ~~buffer name~~
	- ~~buffer position~~
	- buffered keys
- autocomplete
- lsp
