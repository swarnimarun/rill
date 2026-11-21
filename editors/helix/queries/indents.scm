[
  (block)
  (record_type)
  (record_expression)
  (parameters)
  (arguments)
  (parenthesized_expression)
  (match_expression)
  (binary_expression)
  (function_operator_expression)
  (application_expression)
] @indent

["}" ")"] @outdent

; A value beginning below `let` needs an indent on its first line too.
; Same-line block values already supply their own indentation.
(local_binding
  "let" @binding-start
  value: (_) @indent
  (#not-same-line? @indent @binding-start)
  (#set! "scope" "all"))
