
## make lsp server run automatically
An LSP server is usually started when a file it should handle is opened, normally known from its extension.
By using the `lsp` command, it's possible to automatically start an LSP server like that.
For each LSP server you wish to register, add this to one of your config files:
```
lsp "lsp-server-command" "**.ext"
```
Where `"**.ext"` is a glob pattern that, when matched against a buffer just opened,
will invoke the LSP server using `"lsp-server-command"`.
In this case, whenever we open a buffer with the extension `.ext`.

If you need to inspect/debug the protocol messages, you can pass an extra path argument of a log file:
```
lsp "lsp-server-command" "**.ext" my-lsp-server-log.txt
```

You can then open the lsp log at any time with the command `lsp-open-log`.

You can check a full example with many LSP server configured in my
[my config repository](https://github.com/vamolessa/pepper-config/blob/master/init.pp#L3).

## bindings

| binding | expands to | action |
| `K` | `: lsp-hover<enter>` | display hover information (requires a running lsp server) |
| `gd` | `: lsp-definition<enter>` | jumps to where the symbol under the cursor is defined (requires a running lsp server) |
| `gr` | `: lsp-references -context=2<enter>` | lists all references of the symbol under the cursor with 2 lines of context (requires a running lsp server) |
| `gs` | `: lsp-document-symbols<enter>` | lists all symbols in the buffer (requires a running lsp server) |
| `rr` | `: lsp-rename<enter>` | rename the symbol under the cursor (requires a running lsp server) |
| `ra` | `: lsp-code-action<enter>` | suggests possible refactors for the region under the cursor (requires a running lsp server) |
| `rf` | `: lsp-format<enter>` | auto-format the buffer's content (requires a running lsp server) |

## commands

### `lsp`
Automatically starts a lsp server (by running `<lsp-command>`) when a buffer matching a glob `<glob>` is opened.
The lsp command only runs if the server is not already running.
Optionally it's possible to set a log file where all lsp communication will be dumped into by setting `<log-path>`
(it can be latter opened with the `lsp-open-log` command).
- usage: `lsp <lsp-command> <glob> [<log-path>]`

### `lsp-open-log`
If the lsp server associated with the current buffer is logging to a file, opens it as a buffer
(if you want to refresh it, use the `reopen` command).
- usage: `lsp-open-log`

### `lsp-start`
Manually starts a lsp server (by running `<lsp-command>`).
Optionally it's possible to set a log file where all lsp communication will be dumped into by setting `<log-path>`
- usage: `lsp-start <lsp-command> [<log-path>]`

### `lsp-stop`
Stops the lsp server associated with the current buffer.
- usage: `lsp-stop`

### `lsp-stop-all`
Stops all lsp servers.
usage: `lsp-stop-all`

### `lsp-hover`
Displays lsp hover information for the item under the main cursor.
- usage: `lsp-hover`

### `lsp-definition`
Jumps to the location of the definition of the item under the main cursor.
- usage: `lsp-definition`

### `lsp-declaration`
Jumps to the location of the declaration of the item under the main cursor.
- usage: `lsp-declaration`

### `lsp-implementation`
Jumps to the location of the implementation of the item under the main cursor.
- usage: `lsp-implementation`

### `lsp-references`
Opens up a buffer with all references of the item under the main cursor.
Optionally overrides the `<context-len>` (default is `2`). That is: how many lines above and under each reference to show.
- usage: `lsp-references [<context-len>]`

### `lsp-rename`
Renames the item under the main cursor.
- usage: `lsp-rename`

### `lsp-code-action`
Lists and then performs a code action based on the main cursor context.
- usage: `lsp-code-action`

### `lsp-document-symbols`
Pick and jump to a symbol in the current buffer listed by the lsp server.
- usage: `lsp-document-symbols`

### `lsp-workspace-symbols`
Opens up a buffer with all symbols in the workspace found by the lsp server.
Optionally pre-filters results with a `<query>`.
- usage: `lsp-workspace-symbols [<query>]`

### `lsp-format`
Format the whole buffer.
- usage: `lsp-format`
