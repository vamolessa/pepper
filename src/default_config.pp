map -normal <esc> cdcVs<esc>
map -normal <c-c> cdcVs<esc>

map -normal . Qa

map -normal I dgii
map -normal <c-i> dgli
map -normal o dgli<enter>
map -normal O dgii<enter><up>
map -normal J djgivkgli<space><esc>

map -normal K :<space>lsp-hover<enter>
map -normal gd :<space>lsp-definition<enter>
map -normal gr :<space>lsp-references<space>-context=2<enter>
map -normal gs :<space>lsp-document-symbols<enter>
map -normal rr :<space>lsp-rename<enter>
map -normal ra :<space>lsp-code-action<enter>
map -normal rf :<space>lsp-format<enter>

syntax "**/*.pp" {
	keywords = {source|try|macro|syntax|spawn|read-line|pick|add-picker-option|map|lsp}
	symbols = {=|%{|%}}
	literals = {-{%w-_}|{%u%d_}}
	strings = {'{(\')!'.}|"{(\")!".}}
	comments = "#{.}"
	texts = {{%w-_}}
}

syntax "**/*.refs" {
	keywords = {{%a/%._-!:}|%w:{%a/%._-!:}}
	symbols = {,}
	literals = {%d{%w%._}}
	strings = ""
	comments = ""
	texts = {{%w-_}}
}

syntax "**/*.rs" {
	keywords = {fn|let|const|static|if|else|match|loop|while|for|break|continue|return|mod|use|as|in|enum|struct|trait|type|impl|dyn|where|mut|ref|pub|unsafe|extern}
	types = {bool|u8|u16|u32|u64|usize|i8|i16|i32|i64|isize|f32|f64|str|char|%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|:|;|,|=|<|>|+|-|/|*|%%|%!|?|&|%||@}
	literals = {true|false|self|'\.{!'.}|'.'|b'\.{!'.}|b'.'|%d{%w%._}|'%a{%w_}}
	strings = {"{(\")!".}|b"{(\")!".}}
	comments = {//{.}|/*{!(*/).$}}
}

syntax "**/*.{c,h,cpp,hpp}" {
	keywords = {abstract|as|base|break|case|catch|checked|class|const|continue|default|delegate|do|else|enum|event|explicit|extern|finally|fixed|for|foreach|goto|if|implicit|in|interface|internal|is|lock|namespace|new|operator|out|override|params|private|protected|public|readonly|ref|return|sealed|sizeof|stackalloc|static|struct|switch|throw|try|typeof|unchecked|unsafe|using|virtual|volatile|while|add|alias|ascending|async|await|by|descending|dynamic|equals|from|get|global|group|into|join|let|nameof|notnull|on|orderby|partial|remove|select|set|unmanaged|value|var|when|where|yield}
	types = {bool|byte|char|decimal|double|float|int|long|object|sbyte|short|string|uint|ulong|ushort|void|%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@}
	literals = "true|false|this|'\.{!'.}|'.'|%d{%w%._}|#{ }{%a}"
	strings = {"{(\")!".}}
	comments = {//{.}|/*{!(*/).$}}
}

syntax "**/*.cs" {
	keywords = {abstract|as|base|break|case|catch|checked|class|const|continue|default|delegate|do|else|enum|event|explicit|extern|finally|fixed|for|foreach|goto|if|implicit|in|interface|internal|is|lock|namespace|new|operator|out|override|params|private|protected|public|readonly|ref|return|sealed|sizeof|stackalloc|static|struct|switch|throw|try|typeof|unchecked|unsafe|using|virtual|volatile|while|add|alias|ascending|async|await|by|descending|dynamic|equals|from|get|global|group|into|join|let|nameof|not|null|on|orderby|partial|remove|select|set|unmanaged|value|var|when|where|yield}
	types = {bool|byte|char|decimal|double|float|int|long|object|sbyte|short|string|uint|ulong|ushort|void|%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@}
	literals = "true|false|this|'\.{!'.}|'.'|%d{%w%._}|#{%a}"
	strings = {"{(\")!".}}
	comments = {//{.}|/*{!(*/).$}}
}

syntax "**/*.lua" {
	keywords = {and|break|do|else|elseif|end|for|function|if|in|local|not|or|repeat|return|then|until|while}
	symbols = {+|-|*|/|%%|^|#|<|>|=|~|%(|%)|%{|%}|%[|%]|;|:|,|%.|%.%.|%.%.%.}
	literals = {nil|false|true|_G|_ENV|%d{%w%._}}
	strings = {'{(\')!'.}|"{(\")!".}|%[%[{!(%]%]).}}
	comments = {--{.}|--%[%[{!(%]%]).$}}
}

syntax "**/*.md" {
	keywords = "#{.}$"
	symbols = {%||%!|-}
	literals = {%[{!%].}%({!%).}}
	strings = {```{!(```).$}|`{(\`)!`.}}
	texts = {{%w_}}
}

syntax "**/*.html" {
	keywords = {<{%w_-}|</{%w_-}|>|/>}
	types = {%!DOCTYPE}
	symbols = {=}
	strings = {'{(\')!'.}|"{(\")!".}}
	comments = {<%!--{!(-->).$}}
	texts = {{%w-_}}
}

syntax "**/*.js" {
	keywords = {break|export|super|case|extends|switch|catch|finally|class|for|throw|const|function|try|continue|if|typeof|debugger|import|var|default|in|of|void|delete|instanceof|while|do|new|with|else|return|yield|enum|implements|package|public|interface|private|static|let|protected|yield|async|await|abstract|float|synchronized|boolean|goto|throws|byte|int|transient|char|long|volatile|double|native|final|short|arguments|get|set}
	types = {%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@}
	literals = {null|undefined|this|true|false|%d{%w%._}}
	strings = {'{(\')!'.}|"{(\")!".}|`{(\`)!`.}}
	comments = {//{.}|/*{!(*/).$}}
}
