# pepper

### An opinionated modal code editor for your terminal with a focus on programmer's comfort

![main screenshot](screenshots/main.png)

[more gifs](screenshots)

Pepper is an experiment of mine to craft a code editor focused on programmer's comfort.
It's mission is to be a minimal and fast code editor with an orthogonal set of both editing and navigation features.

#### !!! WARNING! Pra-alpha software ahead !!!

### [try the demo](https://matheuslessarodrigues.itch.io/pepper#demo)
Demo's limitations (may change in the future)
- can't change keybindings
- can't save files

### [default keybindings](modes)
### [scripting api](scripting)

### if you find a bug or need help
Please [open an issue](https://github.com/matheuslessarodrigues/pepper/issues)

## goals

- Small set of editing primitives
- Mnemonic and easy to reach default keybindings (assuming a qwerty keyboard)
- Require minimal effort to navigate and explore a workspace
- Cross-plaftorm (Linux, Windows, Max)
- Customizable through scripting
- Extensible through external cli tools
- Be as fast and reponsive as possible

## non goals

- Support every possible workflow (it will never get close to feature parity with vim or emacs)
- Have complex ui (like breadcumbs, floating windows, extra status bars, etc)
- Multiple viewports (you can open multiple clients and they'll be connected)
- Undo tree
- Support for text encodings other than UTF-8
- Fuzzy file picker (you can integrate with fzf)
- Workspace wide search (you can integrate with Grep, Ripgrep, etc)
- Having any other feature that could be better implemented by integrating an external tool

## features

- everything is reachable through the keyboard
- modal editing (normal mode also selects text)
- multiple cursors
- caret style cursors (cursors can move past last line character and text is always inserted to its left)
- text-objects
- macros
- lua scripting
- client/server architecture
- simple syntax highlighting
- support for language server protocol

## philosophy

In the spirit of [Handmade](https://handmade.network/), almost all features are coded from scratch using simple stable Rust code.
These are the only external crates being used in the project (mainly because of crossplatform):
- `ctrlc`: prevents closing application on `ctrl-c` on all platforms
- `crossterm`: crossplatform terminal interaction
- `argh`: process complex cli args. eases rapid prototyping of new cli features
- `polling`: crossplatform socket events
- `mlua`: adds support for lua scripting. could be own new scripting language, however there's value on using a known one
- `fuzzy-matcher`: fuzzy matching for the picker ui. it could be replaced, however it's implementation does not get in the way and has minimal dependencies
- `uds_windows` (windows-only): unix domain sockets for windows

## modal editing

Pepper is modal which means keypresses do different things depending on which mode you're in.
However, it's also designed to have few modes so the overhead is minimal. Most of the time, users will be in
either `normal` or `insert` mode.

## comparing to vim

Like Vim, you have to actively start text selection.
However, unlike it, you can also manipulate selections in normal mode.
Also, there's no 'action' then 'movement'. There's only selections and actions.
That is, `d` will always only delete selected text. If the selection was empty, it does nothing.

Pepper expands on Vim's editing capabilities by supporting multiple cursors.
This enables you to make several text transformations at once.
Also, cursors behave like carets instead of blocks and can always go one-past-last-character-in-line.

## comparing to kakoune

Like Kakoune, you can manipulate selections while in normal mode and actions always operate on selections.
However, unlike it, normal mode remembers if you're selecting text or nor (think a pseudo-mode).
This way, there's no need for extra `alt-` based keybindings.

Pepper is heavily inspired by Kakoune's selection based workflow and multiple cursors.
However its cursors behave like caret ranges instead of block selections.
That is, the cursor is not a one-char selection but only a visual cue to indicate the caret location.

## development thread
It's possible to kinda follow Pepper's development history in this [twitter thread](https://twitter.com/ahvamolessa/status/1276978064166182913)

## big features todo
- language server protocol (in progress)
- debug adapter protocol
