## bindings

| binding | expands to | action |
| `K` | `: lsp-hover<enter>` | display hover information (requires a running lsp server) |
| `gd` | `: lsp-definition<enter>` | jumps to where the symbol under the cursor is defined (requires a running lsp server) |
| `gr` | `: lsp-references -context=2<enter>` | lists all references of the symbol under the cursor with 2 lines of context (requires a running lsp server) |
| `gs` | `: lsp-document-symbols<enter>` | lists all symbols in the buffer (requires a running lsp server) |
| `rr` | `: lsp-rename<enter>` | rename the symbol under the cursor (requires a running lsp server) |
| `ra` | `: lsp-code-action<enter>` | suggests possible refactors for the region under the cursor (requires a running lsp server) |
| `rf` | `: lsp-format<enter>` | auto-format the buffer's content (requires a running lsp server) |

