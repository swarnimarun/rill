(comment) @comment
(string) @string
(c_string) @string
(escape_sequence) @string.escape
(c_escape_sequence) @string.escape
(integer) @number
(boolean) @constant.builtin

(identifier) @variable
((identifier) @constructor
  (#match? @constructor "^[A-Z]"))
(type_identifier (identifier) @type)
(parameter name: (identifier) @variable.parameter)
(legacy_parameter name: (identifier) @variable.parameter)
(extern_parameter name: (identifier) @variable.parameter)
(function_declaration name: (identifier) @function)
(legacy_function_declaration name: (identifier) @function)
(extern_function_declaration name: (identifier) @function)
(extern_constant_declaration name: (identifier) @constant)
(field_declaration name: (identifier) @property)
(record_field name: (identifier) @property)
(record_expression type: (qualified_identifier (identifier) @type .))
(record_expression type: (qualified_identifier (identifier) @module "."))
(field_expression field: (identifier) @property)
(variant name: (identifier) @constructor)
(constructor_pattern name: (qualified_identifier (identifier) @constructor .))
(call_expression function: (qualified_identifier (identifier) @function .))
(call_expression function: (qualified_identifier (identifier) @module "."))
(application_expression function: (identifier) @function)
(apply_expression function: (identifier) @function)
(application_expression function: (parenthesized_expression (identifier) @function))
(apply_expression function: (parenthesized_expression (identifier) @function))
(application_expression function: (field_expression field: (identifier) @function .))
(apply_expression function: (field_expression field: (identifier) @function .))
(local_binding name: (identifier) @function value: (lambda_expression))
(let_expression name: (identifier) @function value: (lambda_expression))
(function_operator_expression operator: "|>" right: (identifier) @function)
(function_operator_expression left: (identifier) @function operator: "<|")
(function_operator_expression left: (identifier) @function operator: [">>" "<<"])
(function_operator_expression operator: [">>" "<<"] right: (identifier) @function)
(import_declaration module: (qualified_identifier (identifier) @module))
(import_declaration alias: (identifier) @module)
(native_directive name: _ @attribute)

((type_identifier (identifier) @type.builtin)
  (#match? @type.builtin "^(i32|u8|u32|u64|f32|bool|unit|str|cstr|ptr|const_ptr|u8x16)$"))
(foreign_type ["i32" "u8" "u32" "u64" "f32" "bool" "unit" "cstr" "ptr" "const_ptr"] @type.builtin)

["fn" "let" "mut" "type" "record" "extern" "const" "import" "export" "as" "read" "inout" "move" "in"] @keyword
["if" "then" "else" "match"] @keyword
["while" "do"] @keyword
["not" "!" "+" "-" "*" "/" "%" "==" "!=" "<" "<=" ">" ">=" "&&" "||" "|>" "<|" ">>" "<<" "<-" "=" "=>" "->"] @operator
["(" ")" "{" "}"] @punctuation.bracket
[";" "," ":" "."] @punctuation.delimiter
"@" @attribute
