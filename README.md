# pepper
Experimental code editor

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
- client/server model
	- ~~dumb client sends Keys receives EditorOperations~~
	- ~~track client that last send message (focused)~~
	- reuse allocation when deserializing EditorOperation::Content
	- show error on focused client
- custom bindings
	- ~~custom bindings expand to builtin bindings~~
	- ~~custom bindings take precedence~~
	- define custom bindings in config file
- config file
	- load config file at startup
	- reload config file when changed??
- command mode
	- ~~basic command mode~~
	- ~~default commands~~
	- define custom commands in config file?? (or just aliases??)
- syntax highlighting
- file operations
	- ~~edit (command to open/create file?)~~
	- ~~save~~
	- ~~reuse buffer if already open~~
	- ~~remove all buffer views (and viewport handles) when closing a buffer~~
- code navigation
	- home/end
	- find char
- status bar
	- buffer name
	- buffer position
- MORE!
