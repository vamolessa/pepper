<!-- {% raw %} -->

Pepper language syntax definitions are minimalistic.
That is, it's just a collection of patterns for each token kind.
These token kinds control which theme color to use when rendering that token.

Here's an example of syntax definition for the lua language:

```
syntax "**/*.lua" {
	keywords = {and|break|do|else|elseif|end|for|function|if|in|local|not|or|repeat|return|then|until|while}
	symbols = {+|-|*|/|%%|^|#|<|>|=|~|%(|%)|%{|%}|%[|%]|;|:|,|%.|%.%.|%.%.%.}
	literals = {nil|false|true|_G|_ENV|%d{%w%._}}
	strings = {'{(\')!'.}|"{(\")!".}|%[%[{!(%]%]).}}
	comments = {--{.}|--%[%[{!(%]%]).$}}
}
```

You can see a full example of language definitions that [come out-of-the-box](https://github.com/vamolessa/pepper/tree/master/src/default_config.pp).

Note that it's not possible to break a pattern in multiple lines and a new syntax definition is loaded,
the token pattern priorities are always:
- keywords
- types
- symbols
- literals
- strings
- comments
- texts

Also, if a syntax can't match a token kind to a text slice, it will assume `text` kind which is used for normal text.
So in theory, when defining a syntax definition, you can skip defining a pattern for the `texts` token kind.
The default pattern for text tokens is `%a{%w_}|_{%w_}` which is the rule most languages use for their identifiers.

## token patterns
Pepper uses it's own syntax to define patterns. It's inspired by both lua patterns and simple regexes, however the
syntax was designed so it's simpler to compile and the interpreter is not recursive.

### pattern syntax

| subpattern | matches |
| --- | --- |
| `<char>` | matches a character (except `%$.!()[]{}` that need escaping) |
| `%a` | matches an alphabetic character |
| `%l` | matches a lowercase character |
| `%u` | matches an uppercase character |
| `%d` | matches a single digit |
| `%w` | matches an alphanumeric character |
| `$` | matches line end |
| `.` | matches any character |
| `%%` | matches `%` |
| `%$` | matches `$` |
| `%.` | matches `.` |
| `%!` | matches `!` |
| `%(` | matches `(` |
| `%)` | matches `)` |
| `%[` | matches `[` |
| `%]` | matches `]` |
| `%{` | matches `{` |
| `%}` | matches `}` |
| `[ ... ]` | matches any of these subpatterns |
| `[! ... ]` | matches anything except these subpatterns |
| `( ... )` | matches a sequence of subpatterns |
| `(! ... )` | matches anything except this sequence of subpatterns |
| `{ ... }` | tries to match any of these subpatterns as much as possible |
| `<pipe>` | if what came before it fails, try again from the beginning with the new pattern to the right kinda like an 'or' |

### group subpatterns `[ ... ]`
This will try to match each subpattern inside the brackets in declaration order.
As soon as one of them matches, the pattern continues by jumping to after the `]`. If all of them fails, then
this group subpattern also fails. If right after `[` there is a `!`, then the logic is inverted and this group
subpattern only matches if every subpattern inside it fails, and fails if any of them matches.

#### examples

| pattern | matches | does not match |
| --- | --- | --- |
| `[abc]` | `b`, `c` | `d`, `3` |
| `x[abc]y` | `xay`, `xby` | `xy`, `xdy` |
| `[!abc]` | `d`, `8` | `a`, `b` |

### sequence subpatterns `( ... )`
A sequence attempts to match each subpattern inside the brackets in declaration order.
As soon as one of them fails, the sequence fails. If right after `(` there is a `!`, then the logic
is inverted and this sequence subpattern will fail if all subpatterns inside it match. On the other hand,
given that the sequence has size `n`, it will only match if the next `n` chars do not match the sequence;
however consuming those `n` chars.

#### examples

| pattern | matches | does not match |
| --- | --- | --- |
| `(abc)` | `abc` | `ab`, `ab2` |
| `(!abc)` | `ab4` | `ab`, `abc` |

### repeat subpatterns `{ ... }`
This subpattern will try to match each of the subpatterns inside the brackets in declaration order. If any
of them matches, it will repeat and try again from the `{`. If none matches, the pattern continues by jumping
to after the `}`. If any of the subpatterns inside it is prefixed by `!` it is treated as an 'exit pattern'
and, if it matches, the pattern exits this repeat subpattern. If a repeat subpattern contains a `!` inside it,
it will only match if one of the 'exit pattern' matches.

#### examples

| pattern | maatches | does not match |
| --- | --- | --- |
| `{a}b` | `b`, `ab`, `aaab` | `c` |
| `{ab}c` | `ac`, `bc`, `abbbabbbc` | `5` |
| `{ab!c}` | `c`, `abbabc` | `ababa` |

### common patterns

| pattern | description |
| --- | --- |
| `%a{%w_}` | most languages identifiers can be matched with this |
| `%d{%w%._}` | poor-man's number literal parser. as soon as a digit is found, it will parse all alphanumeric char, `.` or `_` in the sequence |
| `"{(\")!".}` | string delimited by `"` which can contain escaped `\"`. note that `(\")` is checked first so it can be ignored correctly |
| `//{.}` | c-style single line comment. will match everything to the right of the `//` |
| `/*{!(*/).$}` | c-style multi line comment. the order inside `{}` is important. the 'exit pattern' comes first to stop as soon as a `*/` is found |

<!-- {% endraw %} -->