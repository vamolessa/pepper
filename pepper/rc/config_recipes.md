Here you'll find snippets for common pepper solutions.

If you want to see a full example config folder for pepper, check [my config repository](https://github.com/vamolessa/pepper-config).

## load config file on startup
Since pepper does not try to load config files from a specific folder at startup,
the best way to emulate this is by creating an alias in your shell profile.

```
# unix shell
alias pp='pepper --config ~/.config/.pepper'

# windows cmd
doskey /exename=cmd.exe pp=pepper --config "%HOMEDRIVE%%HOMEPATH%/.pepper" $*
```

With this, whenever you type `pp`, pepper will start and immediately source all the commands
put inside the `.pepper` file in your home directory.
This way you're in control over from where pepper fetches its config files.

### per project config
It's also posssible to setup configs that are per project.
Place a config file (`.pepper` in this example) in the root directory of your project then change your alias like this:

```
# unix shell
alias pp='pepper --config ~/.config/.pepper --config! .pepper'

# windows cmd
doskey /exename=cmd.exe pp=pepper --config "%HOMEDRIVE%%HOMEPATH%/.pepper" --config! .pepper $*
```

When invoking `--config` with a `!`, it will not generate an error when the file is not found.

**NOTE**: `--config` (and `--config!`) are repeatable. Thus, they can be used to load configs files at different locations.
Also, the files are sourced in the order they appear in the command line.

## keybindings
You can remap keys with the [`map` command](command_reference.md#map) command.

With this, you can hit `<c-s>` to save a buffer to a file while in `normal` mode:
```
# The `<space>` after `:` makes it so the `save` command is not added to the command history
map normal <c-s> :<space>save<enter>
```

If you wish to see all the keybindings that are created by default, you can see the builtin
[default bindings](default_bindings.pepper).

## `save-quit` and `save-quit-all` commands
You can have a 'save and quit' command and a 'save all and quit' command by adding these lines to your pepper config:

``` 
# a command that calls `save` passing it all of its arguments and then calls quit
#
# if there's still unsaved work, the `quit` call will fail (unless you call it with `!`)
command save-quit @{
    save @arg(*)
    quit@arg(!)
}
# `sq` alias
command sq @{ save-quit@arg(!) @arg(*) }

# a command that calls `save-all` and then calls `quit`
command save-quit-all @{
    save-all
    quit@arg(!)
}
# `sqa` alias
command sqa @{ save-quit-all@arg(!) @arg(*) }
```

## auto close brackets
Add this snippet your pepper config to auto close these brackets when insert mode:

```
map insert "(" "()<left>"
map insert "[" "[]<left>"
map insert "{" "{}<left>"
map insert "'" "''<left>"
map insert '"' '""<left>'
map insert "`" "``<left>"
```

## fuzzy file find
Pepper ships with a simple fuzzy file finder (bound to `<space>o`) that uses a file finder binary available on each platform
(`find` on unix and `dir` on windows).

However, it's possible to customize it by rebinding `<space>o` to another command.
For example, if you wish to use [`fd`](https://github.com/sharkdp/fd) instead, you can:

```
command -find-file @{
    picker-entries-from-lines "fd -tf ."
    pick "open:" @{
        open "@picker-entry()"
    }
}
map normal <space>o :<space>-find-file<enter>
```

Here we create a new command `-find-file` which will be called on `<space>o`.
When invoked, it will fill picker entries from the output lines of the command line `fd -tf .`
and then will prompt the user to pick and entry. Once selected, we try to open that file.

## simple pattern finder (like grep)
Pepper ships with a simple pattern finder (bound to `<space>f`) that uses a pattern finder binary available on each platform
(`grep` on unix and `findstr` on windows).

However, it's possible to customize it by rebinding `<space>f` to another command.
For example, if you wish to use [`ripgrep`](https://github.com/BurntSushi/ripgrep) instead, you can:

```
command -find-pattern @{
    readline "find:" @{
        open scratch "@readline-input().refs"
        enqueue-keys aad
        replace-with-output 'rg --no-ignore-global --line-number "@readline-input()"'
    }
}
```

Here we create a new command `-find-pattern` which will be called on `<space>f`.
When invoked, it will prompt the user for a search pattern. Once submitted, such pattern is fed to `rg` whose
output is then pasted into the newly opened buffer which has the same name as the search pattern.

## vim bindings
These mappings somewhat emulate basic vanilla vim keybindings.
However please take note that this will not correctly emulate vim's visual mode,
some builtin features may become inaccessible without further tweakings and, obviously,
the experience will *not* be the same of using vim.

Please remember that if you need 100% vim compatibility, it's just better to simply use vim.

For more keybinding details, check the [builtin keybindigs](bindings.md).

```
map normal gg gk
map normal G gj

map normal $ gl
map normal ^ gi
map normal 0 gh

map normal <c-o> <c-p>
map normal <c-i> <c-n>

map normal f ]]
map normal F [[
map normal t ][
map normal T []
map normal ; }
map normal , {

map normal { <c-k>
map normal } <c-j>

map normal / s
map normal ? s
map normal N p
map normal * Nn
map normal # Pp

map normal a li
map normal A gli
map normal <c-r> U

map normal p Y

map normal zt zk
map normal zb zj

map normal ys y
map normal yy Vy
map normal yiw Awy<esc>
map normal yaw awy<esc>
map normal yi( a(y<esc>
map normal ya( A(y<esc>
map normal yi[ a[y<esc>
map normal ya[ A[y<esc>
map normal yi{ a{y<esc>
map normal ya{ A{y<esc>
map normal yi< a<y<esc>
map normal ya< A<y<esc>
map normal yi" a"y<esc>
map normal ya" A"y<esc>
map normal yi' a'y<esc>
map normal ya' A'y<esc>
map normal yi` a`y<esc>
map normal ya` A`y<esc>

map normal ds d
map normal dd Vd
map normal diw Awd
map normal daw awd
map normal di( a(d
map normal da( A(d
map normal di[ a[d
map normal da[ A[d
map normal di{ a{d
map normal da{ A{d
map normal di< a<d
map normal da< A<d
map normal di" a"d
map normal da" A"d
map normal di' a'd
map normal da' A'd
map normal di` a`d
map normal da` A`d

map normal cs i
map normal cc ci
map normal ciw Awi
map normal caw awi
map normal ci( a(i
map normal ca( A(i
map normal ci[ a[i
map normal ca[ A[i
map normal ci{ a{i
map normal ca{ A{i
map normal ci< a<i
map normal ca< A<i
map normal ci" a"i
map normal ca" A"i
map normal ci' a'i
map normal ca' A'i
map normal ci` a`i
map normal ca` A`i
```
