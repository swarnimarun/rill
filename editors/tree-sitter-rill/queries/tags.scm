(function_declaration name: (identifier) @name) @definition.function
(legacy_function_declaration name: (identifier) @name) @definition.function
(extern_function_declaration name: (identifier) @name) @definition.function
(type_declaration name: (type_identifier) @name) @definition.type
(extern_constant_declaration name: (identifier) @name) @definition.constant
(call_expression function: (qualified_identifier) @name) @reference.call
(application_expression function: (identifier) @name) @reference.call
(apply_expression function: (parenthesized_expression (identifier) @name)) @reference.call
(local_binding name: (identifier) @name value: (lambda_expression)) @definition.function
