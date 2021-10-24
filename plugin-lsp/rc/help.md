
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

