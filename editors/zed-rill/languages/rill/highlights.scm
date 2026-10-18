; Highlights for Rill. Names are left unclassified: telling a type from a function
; from a local needs the resolver, and the server reports that through semantic
; tokens instead of the grammar guessing here.

["let" "var" "set" "if" "else" "match" "for" "when" "yield" "type"
 "record" "trait" "impl" "project" "use" "comptime" "unsafe"
 "inout" "sink" "dyn" "true" "false"] @keyword

["option" "one" "all" "sync" "race" "rush" "branch"] @function.special

(number) @number
(string) @string
(raw_string) @string.special
(character) @string.escape
(boolean) @constant

(line_comment) @comment
(block_comment) @comment

["+" "-" "*" "/" "%" "++" "==" "!=" "<" ">" "<=" ">=" "and" "or" "|" "=" ":=" "->" "::"]
@operator

["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";" ":" "."] @punctuation.delimiter
