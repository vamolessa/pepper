# recipes
Here you'll find snippets for common solutions for pepper.

If you want to see an example config folder for pepper, check [my config repository](https://github.com/matheuslessarodrigues/pepper-config).

## load config file on startup
Since pepper won't load a config file by itself on startup, the easiest way to achieve this
is by creating an alias in your shell profile.

```
alias pp='pepper -c ~/.config/pepper/init.lua'
```

With this, whenever you type `pp`, pepper will load with the configs you put inside the file `~/.config/pepper/init.lua`.
This is better because you're in control over not only when pepper loads configs from the disk but also from where it fetches
is configs.

## multi-file config
It's possible to load other lua files when executing a config script.
By using `script.source("filename.lua")`, you can load a new script relative to the current file.

`script.directory()` will return the directory from where this script was loaded. Beware, though, as it may surprise you
when calling a function from another script. It's best to always save the result of this function to a local variable and read from there.

Also, lua modules are available too so you can `require` modules in your system.

## simple keybinds
You can remap keys with the [`keymap`](../scripting#keymap) functions.

With this, you can hit `<c-s>` to save a buffer to a file:
```lua
keymap.normal("<c-s>", ":s<enter>")
```

## language syntax lazy loading
It's possible to only load a language syntax when a buffer containing a source code for that language first loads.
This will prevent pepper from doing unnecessary work on startup for languages that will never be used in that session.

When a new buffer is loaded or saved with a new path, its path is checked agains a cached glob that checks its extension
for supported languages and, if it matches, it then loads the file `<script-name>.lua` that should contain configurations
specific to that language. Feel free to change the supported language list!

For more details on how to define a language syntax, check the [syntax definition page](../syntax).

```lua
local directory = script.directory(); -- remembers the directory from where this very script as loaded
local langs = {}
for i,ext in ipairs({"rs", "lua", "cs", "js", "html", "md"}) do
	langs[#langs + 1] = {
		loaded = false, -- remembers if this language was already loaded
		glob = glob.compile("**/*." .. ext),
		path = directory .. "/" .. ext .. ".lua", -- will look for a file named 'ext.lua' in the same directory
	}
end

function try_load_language(buffer_handle)
	-- will check each not loaded language if the current buffer's path
	-- matches the language's glob
	for i, lang in ipairs(langs) do
		if not lang.loaded then -- only do work if not loaded
			if buffer.path_matches(lang.glob, buffer_handle) then
				lang.loaded = true
				script.source(lang.path) -- source language script
				return
			end
		end
	end
end

buffer.on_load(try_load_language) -- try to load language when a new buffer is loaded
buffer.on_save(function(buffer_handle, new_path) -- try to load language when buffer changes its path
	if new_path then
		try_load_language(buffer_handle)
	end
end)
```

## run shell commands with `!`
While in normal mode, you'll be able to enter 'run shell command' mode by pressing `!`.
Their output will be printed to the status bar.

**NOTE**: this requires a bash script interpreter. On windows, I use `ash` that comes in busybox.

```lua
function run_shell()
	read_line.prompt("!")
	read_line.read(function(command)
		if command == nil then
			return
		end
	
		local stdout = process.pipe("sh", {"-c", command}) -- when this returns, the process will have finished
		print(stdout)
	end)
end
keymap.normal("!", ":run_shell<enter>")
```

## simple fzf integration (windows)
This uses `conhost` to launch a new window with fzf which then tells the focused pepper client which file to open.
While in normal mode, you can launch fzf picker with `<c-o>`.

**NOTE**: this snippet also requires `fd` to list files and `xargs` to correctly supply pepper with the correct cli arguments.
This will be better once Windows Terminal has support for communicating with already running instances.

```lua
function fzf()
	-- this command will pipe fd to fzf then, when it returns, it will call pepper with '--as-focused-client'
	-- which will let us pass commands to the currently focused client (the one which invoked this function)
	-- in this case, we use it to tell that client to open a new file
	local command = [[fd -tf --path-separator / . | fzf | xargs -rI FILE pepper --as-focused-client "FILE" ]]
	process.spawn("conhost", {"sh", "-c", command})
end
keymap.normal("<c-o>", ":fzf()<enter>")
```

## simple find file (no fzf)
This uses `fd` to feed file names to the picker ui which then will let you choose a file to open.
While in normal mode, you can find files with `<c-o>`.

```lua
function find_file()
	picker.reset()
	picker.prompt("open:")
	
	local picked = false
	process.spawn("fd", {"-tf", "--path-separator", "/", "."}, nil, function(output)
		-- this callback is called whenever there's new output from the spawned process
		-- and once more at the end with 'output = nil' to indicate that the process finished
		if picked or output == nil then
			return
		end
		-- iterate over 'output' lines
		for file in string.gmatch(output, "[^\r\n]+") do
			picker.entry(file)
		end
	end)
	
	picker.pick(function(file)
		picked = true
		-- if a file was picked, open it
		if file then
			buffer.open(file)
		end
	end)
end
keymap.normal("<c-o>", ":find_file()<enter>")
```

## simple ripgrep integration
While in normal mode, open a find in workspace readline prompt with `<c-f>` which will feed
into ripgrep. Then its find results will be shown in a picker prompt. Selecting a find entry
will take you to that file and line location.

**NOTE**: for performance reasons, this integration will only search files with the same extension
as the current buffer (if there's one).

```lua
function ripgrep()
	read_line.prompt("rg:")
	-- first a search pattern is read from the user
	read_line.read(function(search_pattern)
		-- early return if action was canceled
		if search_pattern == nil then
			return
		end
		
		picker.reset()
		picker.prompt("jump:")
		
		local args = {"--line-number"}
		local buffer_path = buffer.path()
		-- maybe restrict searched files based on their extension
		if buffer_path ~= nil then
			local extension = string.match(buffer_path, "[^%.]%.(%w+)$")
			if extension ~= nil then -- only search files with the same extension as current buffer
				args[#args + 1] = "--type-add"
				args[#args + 1] = "t:*." .. extension
				args[#args + 1] = "-tt"
			end
		end
		args[#args + 1] = search_pattern
		
		local picked = false
		process.spawn("rg", args, nil, function(output)
			-- this callback is called whenever there's new output from the spawned process
			-- and once more at the end with 'output = nil' to indicate that the process finished
			if picked or output == nil then
				return
			end
			-- iterate over 'output' lines
			for match in string.gmatch(output, "[^\r\n]+") do
				local file, line, text = string.match(match, "([^:]+):([^:]+):%s*(.*)")
				picker.entry(file .. ":" .. line, text)
			end
		end)
		
		picker.pick(function(file_and_line)
			picked = true
			-- early return if no file was picked
			if file_and_line == nil then
				return
			end
			local file, line = string.match(file_and_line, "([^:]+):([^:]+)")
			buffer.open(file, line)
		end)
	end)
end
keymap.normal("<c-f>", ":ripgrep()<enter>")
```
