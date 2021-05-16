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
It's possible to recursively load config files from a config file by using the [`source`](command_reference.md#source) command.

```
# in your `init.pp`

source "other-config.pp"
source "some-other-config.pp"
```

## keybindings
You can remap keys with the [`keymap`](command_reference.md#map) command.

With this, you can hit `<c-s>` to save a buffer to a file:
```
# The `<space>` after `:` makes it so the `save` command is not added to the command history
map -normal <c-s> :<space>save<enter>
```

If you wish to see all the keybindings that are created by default, you can see the builtin [default config](https://github.com/vamolessa/pepper/blob/master/src/default_config.pp).

## run program with `!`
While in normal mode, you'll be able to enter 'run program' mode by pressing `!`.
Its output will be printed to the status bar.

```
macro run-shell {
	read-line -prompt="!" {
		# this block executes once read-line finishes,
		# and register %z will contain the line read

		spawn %z { # spawn the typed command
			# this block executes once the process finishes,
			# its stdout will also be placed on register %z

			print %z # print process output
		}
	}
}
map -normal ! :<space>run-shell<enter> # bind run-shell macro to `!`
```

## simple fuzzy file opener
This uses [`fd`](https://github.com/sharkdp/fd) to feed file names to the picker ui which then lets you choose a file to open.
While in normal mode, you can invoke it with `<c-o>`.

```
macro fuzzy-open-file {
	spawn "fd -tf -0 --path-separator / ." -split-on-byte=0 {
		# this block executes whenever `fd` returns a new entry (separated by byte 0)
		# those stdout bytes are placed in register %z

		add-picker-option %z # as entries are found, populate the picker ui
	}
	
	# open the picker ui with the prompt "open:"
	pick -prompt="open:" {
		# this block executes once the file is chosen,
		# and register %z will contain its path
	
		open %z # open the chosen file
	}
}
map -normal <c-o> :<space>fuzzy-open-file<enter> # bind fuzzy-open-file macro to `<c-o>`
```

## simple grep
This defines a macro command that will invoke [`ripgrep`](https://github.com/BurntSushi/ripgrep) and then display its results in a new buffer
from where you can jump to the found locations.

You can use it like `:rg MyStruct` and a buffer will open with all the results.
Then you can use pepper's builtin `gf` to jump to a filepath under the cursor.

```
macro rg z {
	# when this macro is invoked, this block executes and
	# the register %z will contain its argument

	# open a temp buffer named "rg-find-results.refs"
	# the ".refs" extension is useful for syntax highlighting
	open -no-history -no-save -no-word-database "rg-find-results.refs"
	
	# delete buffer contents
	execute-keys <esc>aad
	
	# insert text from `rg` stdout when searching for the pattern
	# given as argument to this macro (register %z)
	replace-with-output -split-on-byte=10 "rg --line-number --path-separator / --no-ignore-global %z"
}
```

**NOTE**: you also use the flag `-auto-close` for the [`open`](command_reference.md#open) command.
This will automatically close the ripgrep results buffer once you jump out of it.

## simple buffer format
This command will save the current buffer, then call [`rustfmt`](https://github.com/rust-lang/rustfmt) with
its path as argument. Once `rustfmt` returns, it reloads the buffer contents from file to apply the formatting.
The `ff` keybind will trigger the command while in normal mode.

```
macro format {
	save # save buffer to make sure all changes go to the file system
	%z = buffer-path # save the current buffer path to register %z
	
	# spawn `rustfmt` passing it the current buffer path
	spawn "rustfmt %z" {
		reload # once rustfmt finishes, reload contents from the file system
	}
}
map -normal ff :<space>format<enter> # bind format macro to `ff`
```

**NOTE**: this command may be most useful when defined from a project config
since you probably want to use a different formatter per project.
Also, since you're reloadin the buffer contents, you'll lose the buffer's history.

## vim bindings
These mappings somewhat emulate basic vanilla vim keybindings.
However please take note that this will not correctly emulate vim's visual mode,
some builtin features may become inaccessible without further tweakings and, obviously,
the experience will *not* be the same of using vim.

If you need 100% vim compatibility, simply use vim.

For more details, check the [builtin keybindigs](bindings.md).

```
map -normal gg gk
map -normal G gj

map -normal $ gl
map -normal ^ gi
map -normal 0 gh

map -normal <c-o> <c-p>
map -normal <c-i> <c-n>

map -normal f ]]
map -normal F [[
map -normal t ][
map -normal T []
map -normal ; }
map -normal , {

map -normal { <c-k>
map -normal } <c-j>

map -normal / s
map -normal ? s
map -normal N p
map -normal * Nn
map -normal # Pp

map -normal a li
map -normal A gli
map -normal <c-r> U

map -normal p Y

map -normal zt zk
map -normal zb zj

map -normal ys y
map -normal yy Vy
map -normal yiw Awy<esc>
map -normal yaw awy<esc>
map -normal yi( a(y<esc>
map -normal ya( A(y<esc>
map -normal yi[ a[y<esc>
map -normal ya[ A[y<esc>
map -normal yi{ a{y<esc>
map -normal ya{ A{y<esc>
map -normal yi< a<y<esc>
map -normal ya< A<y<esc>
map -normal yi" a"y<esc>
map -normal ya" A"y<esc>
map -normal yi' a'y<esc>
map -normal ya' A'y<esc>
map -normal yi` a`y<esc>
map -normal ya` A`y<esc>

map -normal ds d
map -normal dd Vd
map -normal diw Awd
map -normal daw awd
map -normal di( a(d
map -normal da( A(d
map -normal di[ a[d
map -normal da[ A[d
map -normal di{ a{d
map -normal da{ A{d
map -normal di< a<d
map -normal da< A<d
map -normal di" a"d
map -normal da" A"d
map -normal di' a'd
map -normal da' A'd
map -normal di` a`d
map -normal da` A`d

map -normal cs i
map -normal cc ci
map -normal ciw Awi
map -normal caw awi
map -normal ci( a(i
map -normal ca( A(i
map -normal ci[ a[i
map -normal ca[ A[i
map -normal ci{ a{i
map -normal ca{ A{i
map -normal ci< a<i
map -normal ca< A<i
map -normal ci" a"i
map -normal ca" A"i
map -normal ci' a'i
map -normal ca' A'i
map -normal ci` a`i
map -normal ca` A`i
```
