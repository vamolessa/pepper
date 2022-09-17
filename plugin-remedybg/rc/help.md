## commands

TODO: update docs!

### `remedybg-spawn`
Spawns a RemedyBG instance optionally opening a `<session>` file
- usage: `remedybg-spawn [<session>]`

### `remedybg-sync-breakpoints`
Syncs all pepper breakpoints to RemedyBG.
Note that generally this is not needed to be called manually as pepper already syncs breakpoints when it idles.
- usage: `remedybg-sync-breakpoints`

### `remedybg-command`
Controls a debugging by sending a `<command>` to an already running instance.
`<command>` is one of:
- `start`: start debbuging target application
- `start-paused`: same as `start` but breaks at 'main'
- `stop`: stop debbuging target application
- `attach`: attaches to an already running target application
- `continue`: resumes target application's execution
- `run-to-cursor`: resumes target application's execution and breaks at current cursor location

- usage: `remedybg-command <command>`

