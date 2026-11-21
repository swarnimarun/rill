(function_declaration body: (_) @function.inside) @function.around
(legacy_function_declaration body: (_) @function.inside) @function.around
(lambda_expression body: (_) @function.inside) @function.around
(type_declaration) @class.around
(record_type) @class.inside
(parameter) @parameter.inside
(extern_parameter) @parameter.inside
(legacy_parameter) @parameter.inside
(comment) @comment.inside
(comment)+ @comment.around
(record_field) @entry.around
(field_declaration) @entry.around
(match_arm) @entry.around
