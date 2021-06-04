map -normal <esc> cdcVs<esc>
map -normal <c-c> cdcVs<esc>

map -normal . Qa

map -normal I dgii
map -normal <c-i> dgli
map -normal ci cvcCglccgii
map -normal o dgli<enter>
map -normal O dgii<enter><up>
map -normal J djgivkgli<space><esc>

map -normal K :<space>lsp-hover<enter>
map -normal gd :<space>lsp-definition<enter>
map -normal gr ": lsp-references -context=2<enter>"
map -normal gs :<space>lsp-document-symbols<enter>
map -normal rr :<space>lsp-rename<enter>
map -normal ra :<space>lsp-code-action<enter>
map -normal rf :<space>lsp-format<enter>

syntax "**/*.pp" {
	keywords = {source|try|macro|syntax|spawn|read-line|pick|add-picker-option|map|lsp}
	symbols = {=|%{|%}}
	literals = {-{%w-_}|%%%l|{%u%d_}}
	strings = {'{(\')!'.}|"{(\")!".}}
	comments = "#{.}"
	texts = {{%w-_}}
}

syntax "**/*.refs" {
	keywords = {{%a/%._-!:}|%w:{%a/%._-!:}}
	symbols = {,}
	literals = {%d}
	strings = ""
	comments = ""
	texts = {{%w-_}}
}

# https://doc.rust-lang.org/reference/keywords.html
syntax "**/*.rs" {
	keywords = {as|break|const|continue|crate|else|enum|extern|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|static|struct|super|trait|type|unsafe|use|where|while|async|await|dyn|abstract|become|box|do|final|macro|override|priv|typeof|unsized|virtual|yield|try|union}
	types = {bool|u8|u16|u32|u64|usize|i8|i16|i32|i64|isize|f32|f64|str|char|%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%!|?|&|%||@}
	literals = {true|false|self|'\''|'\{!'.}|'.'|b'{(\')(\\)!'.}|%d{%d_}%.%w{%w_}|%d{%w_}|'%a{%w_}}
	strings = {"{(\")!".}|b"{(\")!".}}
	comments = {//{.}|/*{!(*/).$}}
}

# https://docs.microsoft.com/en-us/cpp/cpp/keywords-cpp
syntax "**/*.{c,h,cpp,hpp}" {
	keywords = {alignas|alignof|and_eq|and|asm|auto|bitand|bitor|bool|break|case|catch|class|compl|concept|const|const_cast|consteval|constexpr|constinit|continue|co_await|co_return|co_yield|decltype|default|delete|do|dynamic_cast|else|enum|explicit|export|extern|for|friend|goto|if|inline|mutable|namespace|new|noexcept|not_eq|not|operator|or_eq|or|private|protected|public|register|reinterpret_cast|requires|return|sizeof|static|static_assert|static_cast|struct|switch|template|thread_local|throw|try|typedef|typeid|typename|union|using|virtual|volatile|while|xor_eq|xor}
	types = {char|char8_t|char16_t|char32_t|double|float|int|long|short|signed|unsigned|void|wchar_t|%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@}
	literals = "true|false|this|nullptr|'{(\')!'.}|%d{%d_}%.%w{%w_}|%d{%w_}|#{ }{%a}"
	strings = {"{(\")!".}}
	comments = {//{.}|/*{!(*/).$}}
}

# https://docs.microsoft.com/en-us/dotnet/csharp/language-reference/keywords/
syntax "**/*.cs" {
	keywords = {abstract|as|base|break|case|catch|checked|class|const|continue|default|delegate|do|else|enum|event|explicit|extern|finally|fixed|foreach|for|goto|if|implicit|in|interface|internal|is|lock|namespace|new|operator|out|override|params|private|protected|public|readonly|ref|return|sealed|sizeof|stackalloc|static|struct|switch|throw|try|typeof|unchecked|unsafe|using|virtual|volatile|while|add|alias|ascending|async|await|by|descending|dynamic|equals|from|get|global|group|into|join|let|nameof|not|on|orderby|partial|remove|select|set|unmanaged|value|var|when|where|yield}
	types = {bool|byte|char|decimal|double|float|int|long|object|sbyte|short|string|uint|ulong|ushort|void|%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@}
	literals = "true|false|this|null|'{(\')!'.}|%d{%d_}%.%w{%w_}|%d{%w_}|#{%a}"
	strings = {"{(\")!".}}
	comments = {//{.}|/*{!(*/).$}}
}

# https://www.lua.org/manual/5.1/manual.html#2
syntax "**/*.lua" {
	keywords = {and|break|do|elseif|else|end|for|function|if|in|local|not|or|repeat|return|then|until|while}
	symbols = {+|-|*|/|%%|^|#|<|>|=|~|%(|%)|%{|%}|%[|%]|;|%.|:|,|%.|%.%.|%.%.%.}
	literals = {nil|false|true|_G|_ENV|%d{%d_}%.%w{%w_}|%d{%w_}}
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
	keywords = {</{%w_-}|<{%w_-}|>|/>}
	types = {%!DOCTYPE}
	symbols = {=}
	strings = {'{(\')!'.}|"{(\")!".}}
	comments = {<%!--{!(-->).$}}
	texts = {{%w-_}}
}

# https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Lexical_grammar#keywords
syntax "**/*.{js,ts}" {
	keywords = {break|case|catch|class|const|continue|debugger|default|delete|do|else|export|extends|finally|for|function|if|import|in|instanceof|new|return|super|switch|this|throw|try|typeof|var|void|while|witch|yield|enum|implements|interface|let|package|private|protected|public|static|yield|await}
	types = {%u{%w_}}
	symbols = {%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@}
	literals = {null|undefined|this|true|false|%d{%d_}%.%w{%w_}|%d{%w_}}
	strings = {'{(\')!'.}|"{(\")!".}|`{(\`)!`.}}
	comments = {//{.}|/*{!(*/).$}}
}
