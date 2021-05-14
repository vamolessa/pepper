Here you'll find snippets for common pepper solutions.

If you want to see a full example config folder for pepper, check [my config repository](https://github.com/vamolessa/pepper-config).

## load config file on startup
Since pepper does not try to load config files from a specific folder at startup,
the best way to emulate this is by creating an alias in your shell profile.

```
alias pp='pepper --config ~/.config/pepper/init.pp'
```

With this, whenever you type `pp`, pepper will start with sourcing the configs you put inside the file `~/.config/pepper/init.pp`.
This is better because you're in control over not only when pepper loads configs from the disk but also from where it fetches them.

### project config
It's also easy to load, say, configs that are per project. If you determine that all of your projects that you wish to configure
individually, will do so through a file named `project.pp` in its root, you can set your pepper alias:

```
alias pp='pepper --config ~/.config/pepper/init.pp --try-config project.pp'
```

The only difference from `--try-config` to `--config` is that it won't report an error if the config file was not found.

Both `--config` and `--try-config` are repeatable and can be used to load configs from files in different locations. The files are sourced in the order they appear in the command line.

### multi-file config
It's possible to recursively load config files from a config file by using the [`source`](command-reference#source) command.

```
# in your `init.pp`

source "other-config.pp"
source "some-other-config.pp"
```

## keybindings
You can remap keys with the [`keymap`](command-reference#map) command.

With this, you can hit `<c-s>` to save a buffer to a file:
```
# The <space> after ':' makes it so the 'save' command is not added to the command history
map -normal <c-s> :<space>save<enter>
```

If you wish to see all the keybindings that are created by default, you can see the builtin [default config](https://github.com/vamolessa/pepper/blob/master/src/default_config.pp).

## run program with `!`
While in normal mode, you'll be able to enter 'run program' mode by pressing `!`.
Its output will be printed to the status bar.

```
macro run-shell {
	read-line -prompt="!" COMMAND {
		spawn COMMAND OUTPUT {
			print OUTPUT
		}
	}
}
map -normal ! :<space>run-shell<enter>
```

## simple fuzzy file opener
This uses [`fd`](https://github.com/sharkdp/fd) to feed file names to the picker ui which then lets you choose a file to open.
While in normal mode, you can invoke it with `<c-o>`.

```
macro fuzzy-open-file {
	spawn "fd -tf -0 --path-separator / ." -split-on-byte=0 LINE {
		add-picker-option LINE
	}
	pick -prompt="open" OPTION {
		open OPTION
	}
}
map -normal <c-o> :<space>fuzzy-open-file<enter>
```

## simple grep
This defines a macro command that will invoke [ripgrep](https://github.com/BurntSushi/ripgrep) and then display its results in a new buffer
from where you can jump to the found locations.

You can use it like `:rg MyStruct` and a buffer will open with all the results.
Then you can use pepper's builtin `gf` to jump to a filepath under the cursor.

```
macro rg PATTERN {
	open -no-history -no-save -no-word-database "rg-find-results.refs"
	execute-keys <esc>aad # clean the whole buffer
	replace-with-output -split-on-byte=10 "rg --line-number --path-separator / --no-ignore-global PATTERN"
}
```

**NOTE**: you also use the flag `-auto-close` for the [`open`](command-reference#open) command.
This will automatically close the ripgrep results buffer once you jump out of it.

## simple buffer format (rustfmt)
This command will send the entire buffer's content to [rustfmt](https://github.com/rust-lang/rustfmt) (but it works for any other code formatter).
Then it will replace the buffer's content with its output.
The `ff` keybind will trigger the command while in normal mode.

```
macro format {
	execute-keys <esc>aa # selec the whole buffer
	replace-with-output -pipe "rustfmt"
}
map -normal ff :<space>format<enter>
```

**NOTE**: this command may be most useful when defined from a project config since you probably want to use a different formatter per project.