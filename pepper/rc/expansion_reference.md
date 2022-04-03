Expansions are text substitutions that happen right before a command is evaluated.
They have the form: `@expansion-name(expansion-args)`.

`expansion-args`'s semantics depend on which expansion is being handled.
If an expansion fails to parse its argument, it will not expand and its text will remain untouched.

Expansions can occur as a standalone argument to a command or inside a quoted (`"..."`, `'...'` or `{...}`) argument.
However, in the quoted case, it's possible to disable expansions altogether by adding a `@` to the quote.
That is: `@"..."`, `@'...'` or `@{...}`.

# expansions

## `arg`
When used inside a command declared using the `command` command, expands to the `<index>`th argument it received whell called.
If instead of a zero-based index, its argument is `!`, it expands to `!` if the command was called with a bang
(that is, `command! ...`) and to `` (empty) otherwise.
Also, this expansion's argument can be `*` which will make it expand to all arguments passed to the called command.
In general, `@arg(*)` do not play well inside quoted arguments and are better used when creating command aliases.
That is, something akin to `command my-alias @{ my-alised-command@arg(!) @arg(*) }`.
- usage: `@arg(<index>)` `@arg(!)` `@arg(*)`

## `client-id`
The zero-based id of the current editor client.
Note that a client id of 3, does not imply that there are other 3 clients present (0, 1 and 2)
as they may be no longer active ids.
- usage: `@client-id()`

## `buffer-id`
The zero-based id of the current buffer.
Note that a buffer id of 3, does not imply that there are other 3 buffers present (0, 1 and 2)
as they may be no longer active ids.
- usage `@buffer-id()`

## `buffer-path`
The buffer path as it appears in the statusbar of the current buffer or of the buffer with id `<id>`.
If there is no such buffer, it results in an empty expansion.
- usage: `@buffer-path()` `@buffer-path(<id>)`

## `buffer-absolute-path
The absolute path of the current buffer or of the buffer with id `<id>`.
If there is no such buffer, it results in an empty expansion.
- usage: `@buffer-absolute-path()` `@buffer-absolute-path(<id>)`

## `buffer-content`
All the text content inside the current buffer or of the buffer with id `<id>`.
If there is no such buffer, it results in an empty expansion.
Lines are always separated by `\n`.
- usage: `@buffer-content()` `@buffer-content(<id>)`
