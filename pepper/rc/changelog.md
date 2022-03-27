# changelog

## 0.25.0
- new variable expansion mechanism when evaluating commands
- command aliases that start with `-` won't show up in auto completions

## 0.24.0
- handle buffer paths beginning with `./` (on `Buffer::set_path` and `Buffer::find_with_path`)
- command `$` is now `!` and what was `!` is now removed; that is, there's no longer a 'only insert from command output', just 'replace with command output' (`|` command) and if the selection is empty, it behaves as if it was the old `!`

## 0.23.3
- fix failing lsp protocol test that should only run on windows
- force redeploy on github actions

## 0.23.2
- fix URI parsing on windows

## 0.23.1
- fix crash after pc wakeup on linux (possibly on bsd and mac as well)
- fix server occasionally dropping writes to client on linux

## 0.23.0
- changed default clipboard linux interface to `xclip` instead of `xsel`
- fix crash when `lsp-references` would not load the context buffer
- handle `<c-i>` (insert at end of line) by instead mapping it to tab on unix
- fix some lsp operations not working on unix due to poor path handling

## 0.22.0
- added quit instruction to the start screen
- added '%t' to patterns to match a tab ('\t')
- fix bad handling of BSD's resize signal on kqueue

## 0.21.0
- prevent deadlocks by writing asynchronously to clients from server
- fix possible crash when listing lsp item references when there's a usage near the end of the buffer
- added instructions on how to package the web version of the editor
- added error to `lsp-stop` and `lsp-stop-all` when there is no lsp server running

## 0.20.0
- use builtin word database for completions when plugins provide no suggestions
- prevent closing all clients when the terminal from which the server was spawned is closed
- fix debugging crash when sometimes dropping a client connection

## 0.19.3
- added changelog! you can access it through `:help changelog<enter>`
- added error number to unix platform panics
- fix event loop on bsd
- fix idle events not triggering on unix
- fix buffer history undo crash when you undo after a "insert, delete then insert" single action
- fix messy multiple autocomplete on the same line
- fix crash on macos since there kqueue can't poll /dev/tty

## 0.19.2 and older
There was no official changelog before.
However, up to this point, we were implementing all features related to the editor's vision.
Then fixing bugs and stabilizing the code base.
