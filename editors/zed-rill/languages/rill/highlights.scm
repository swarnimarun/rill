; Highlights for Rill. Unclassified names stay unclassified: telling a type
; from a function from a local needs the resolver, and the server reports that
; through semantic tokens instead of the grammar guessing here. Names that the
; grammar itself places in a role (a declaration's name, a parameter, a field,
; a type argument) are captured, because their role is syntactic.

; ---- keywords ---------------------------------------------------------

["let" "var" "set" "if" "else" "match" "for" "while" "when" "yield"
 "type" "record" "trait" "impl" "project" "use" "extern" "comptime" "unsafe"
 "inout" "sink" "dyn" "as" "is" "true" "false" "static" "threadlocal"] @keyword

(break_statement) @keyword

["not" "size" "shapeof" "reshape"] @keyword.operator

; The handler/builder forms (`option { ... }`, `all { ... }`, `sync { ... }`) are
; names applied to a block; the grammar marks that shape, so the name is a
; function-like special form rather than an ordinary identifier.
(handler name: (identifier) @function.special)

; ---- declarations -----------------------------------------------------

(function_definition name: (identifier) @function)
(function_signature name: (identifier) @function)
(bare_signature name: (identifier) @function)
(project_definition name: (identifier) @function.method)

(type_definition name: (identifier) @type)
(trait_definition name: (identifier) @type)
(type_application name: (identifier) @type)
(type_path (identifier) @type)
(type_parameter (identifier) @type)

(variant name: (identifier) @constructor)
(member name: (identifier) @property)

(parameter name: (identifier) @variable.parameter)
(variable_declaration name: (identifier) @variable)
(let_stmt name: (identifier) @variable)
(for_statement name: (identifier) @variable)

(field_expression (identifier) @property)

; ---- literals ---------------------------------------------------------

(number) @number
(string) @string
(raw_string) @string.special
(character) @string.escape
(boolean) @constant

; ---- trivia -----------------------------------------------------------

(line_comment) @comment
(block_comment) @comment

; ---- operators and punctuation ---------------------------------------

(operator) @operator
[":=" "->" "::"] @operator

["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";" ":" "."] @punctuation.delimiter
