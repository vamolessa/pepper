map-normal <esc> cdcVs<esc>
map-normal <c-c> cdcVs<esc>

map-normal . Qa

map-normal I dgii
map-normal <c-i> dgli
map-normal ci cvcCglccgii
map-normal o dgli<enter>
map-normal O dgii<enter><up>
map-normal J djgivkgli<space><esc>

map-normal K :<space>lsp-hover<enter>
map-normal gd :<space>lsp-definition<enter>
map-normal gr :<space>lsp-references<enter>
map-normal gs :<space>lsp-document-symbols<enter>
map-normal rr :<space>lsp-rename<enter>
map-normal ra :<space>lsp-code-action<enter>
map-normal rf :<space>lsp-format<enter>

alias h help
alias q quit
alias qa quit-all
alias o open
alias s save
alias sa save-all
alias r reopen
alias ra reopen-all
alias c close
alias ca close-all

syntax-begin "**/*.refs"
syntax-keywords "%w:{%a/%._-!:}|{%a/%._-!:}"
syntax-symbols ","
syntax-literals "%d"
syntax-texts "{%w-_}"
syntax-end

#; https://doc.rust-lang.org/reference/keywords.html
#[syntax]
#glob=**/*.rs
#keywords=as|break|const|continue|crate|else|enum|extern|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|static|struct|super|trait|type|unsafe|use|where|while|async|await|dyn|abstract|become|box|do|final|macro|override|priv|typeof|unsized|virtual|yield|try|union
#types=bool|u8|u16|u32|u64|usize|i8|i16|i32|i64|isize|f32|f64|str|char|%u{%w_}
#symbols=%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%!|?|&|%||@
#literals=true|false|self|'\''|'\{!'.}|'.'|b'{(\')(\\)!'.}|%d{%d_}%.%w{%w_}|%d{%w_}|'%a{%w_}
#strings="{(\")!".}|b"{(\")!".}
#comments=//{.}|/*{!(*/).$}
#
#; https://docs.microsoft.com/en-us/cpp/cpp/keywords-cpp
#[syntax]
#glob=**/*.{c,h,cpp,hpp}
#keywords=alignas|alignof|and_eq|and|asm|auto|bitand|bitor|bool|break|case|catch|class|compl|concept|const|const_cast|consteval|constexpr|constinit|continue|co_await|co_return|co_yield|decltype|default|delete|do|dynamic_cast|else|enum|explicit|export|extern|for|friend|goto|if|inline|mutable|namespace|new|noexcept|not_eq|not|operator|or_eq|or|private|protected|public|register|reinterpret_cast|requires|return|sizeof|static|static_assert|static_cast|struct|switch|template|thread_local|throw|try|typedef|typeid|typename|union|using|virtual|volatile|while|xor_eq|xor
#types=char|char8_t|char16_t|char32_t|double|float|int|long|short|signed|unsigned|void|wchar_t|%u{%w_}
#symbols=%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@
#literals=true|false|this|nullptr|'{(\')!'.}|%d{%d_}%.%w{%w_}|%d{%w_}|#{ }{%a}
#strings="{(\")!".}
#comments=//{.}|/*{!(*/).$}
#
#; https://docs.microsoft.com/en-us/dotnet/csharp/language-reference/keywords/
#[syntax]
#glob=**/*.cs
#keywords=abstract|as|base|break|case|catch|checked|class|const|continue|default|delegate|do|else|enum|event|explicit|extern|finally|fixed|foreach|for|goto|if|implicit|in|interface|internal|is|lock|namespace|new|operator|out|override|params|private|protected|public|readonly|ref|return|sealed|sizeof|stackalloc|static|struct|switch|throw|try|typeof|unchecked|unsafe|using|virtual|volatile|while|add|alias|ascending|async|await|by|descending|dynamic|equals|from|get|global|group|into|join|let|nameof|not|on|orderby|partial|remove|select|set|unmanaged|value|var|when|where|yield
#types=bool|byte|char|decimal|double|float|int|long|object|sbyte|short|string|uint|ulong|ushort|void|%u{%w_}
#symbols=%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@
#literals=true|false|this|null|'{(\')!'.}|%d{%d_}%.%w{%w_}|%d{%w_}|#{%a}
#strings="{(\")!".}
#comments=//{.}|/*{!(*/).$}
#
#; https://www.lua.org/manual/5.1/manual.html#2
#[syntax]
#glob=**/*.lua"
#keywords=and|break|do|elseif|else|end|for|function|if|in|local|not|or|repeat|return|then|until|while
#symbols=+|-|*|/|%%|^|#|<|>|=|~|%(|%)|%{|%}|%[|%]|;|%.|:|,|%.|%.%.|%.%.%.
#literals=nil|false|true|_G|_ENV|%d{%d_}%.%w{%w_}|%d{%w_}
#strings='{(\')!'.}|"{(\")!".}|%[%[{!(%]%]).}
#comments=--{.}|--%[%[{!(%]%]).$}
#
#; https://docs.python.org/3/reference/lexical_analysis.html#keywords
#[syntax]
#glob=**/*.py
#keywords=and|as|assert|async|await|break|class|continue|def|del|elif|else|except|finally|for|from|global|if|import|in|is|lambda|nonlocal|not|or|pass|raise|return|try|while|with|yield
#symbols=+|-|*|/|%%|<|>|=|~|%(|%)|%{|%}|%[|%]|;|%.|:|,|%.
#literals=None|False|True|%d{%d_}%.%w{%w_}|%d{%w_}
#strings='{(\')!'.}|"{(\")!".}
#comments=#{.}
#
#; https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Lexical_grammar#keywords
#[syntax]
#glob=**/*.{js,ts}
#keywords=break|case|catch|class|const|continue|debugger|default|delete|do|else|export|extends|finally|for|function|if|import|in|instanceof|new|return|super|switch|this|throw|try|typeof|var|void|while|witch|yield|enum|implements|interface|let|package|private|protected|public|static|yield|await
#types=%u{%w_}
#symbols=%(|%)|%[|%]|%{|%}|%.|:|;|,|=|<|>|+|-|/|*|%%|%.|%!|?|&|%||@
#literals=null|undefined|this|true|false|%d{%d_}%.%w{%w_}|%d{%w_}
#strings='{(\')!'.}|"{(\")!".}|`{(\`)!`.}
#comments=//{.}|/*{!(*/).$}
#
#[syntax]
#glob=**/*.ini
#keywords=%[{!%].}
#symbols==
#comments=;{.}
#texts={%w-_}
#
#[syntax]
#glob=**/*.md
#keywords=#{.}$
#symbols=%||%!|-
#literals=%[{!%].}%({!%).}
#strings=```{!(```).$}|`{(\`)!`.}
#texts={%w-_}
#
#[syntax]
#glob=**/*.html
#keywords=%!DOCTYPE
#symbols==
#strings='{(\')!'.}|"{(\")!".}
#comments=<%!--{!(-->).$}
#texts={%w-_}
#
