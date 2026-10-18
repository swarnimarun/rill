// A grammar for Rill, for editor highlighting and structural selection rather than
// for parsing semantics: the compiler's parser remains the reference, and this one
// deliberately accepts a superset (any expression applied to anything) so that a
// half-typed line still highlights instead of collapsing into an error node.
module.exports = grammar({
  name: 'rill',

  extras: $ => [/\s/, $.line_comment, $.block_comment],

  // For highlighting, resolving these by declaration is right: the parser picks one
  // reading and the queries colour it, and the compiler's own parser remains the
  // authority on what a program means.
  conflicts: $ => [
    [$._expression, $.try_expression],
    [$.match_expression, $.operator],
    [$.set_statement, $.operator],
    [$.application],
    [$.conditional],
    [$._body, $._operand, $._expression],
    [$._operand, $._expression],
    [$._operand, $._expression, $.binary_expression],
    [$.parameter_list, $.tuple_expression],
    [$._body, $._expression],
    [$._body, $._operand],
  ],

  word: $ => $.identifier,

  rules: {
    source_file: $ => seq(
      repeat($.use_declaration),
      repeat($._item),
    ),

    use_declaration: $ => seq('use', $.path, ';'),

    _item: $ => choice(
      $.function_definition,
      $.type_definition,
      $.trait_definition,
      $.impl_definition,
      $.project_definition,
      $.statement,
    ),

    function_definition: $ => seq(
      'let', field('name', $.identifier), $.parameter_list,
      '->', $._type, optional($.effect_row), '=', $._body,
      optional(';'),
    ),

    type_definition: $ => seq(
      'type', field('name', $.identifier), '=', $.type_body, ';',
    ),

    type_body: $ => choice($.record_body, $._type),

    record_body: $ => seq('record', '{', repeat($.member), '}'),

    trait_definition: $ => seq('trait', $.identifier, '{', repeat($.member), '}'),

    impl_definition: $ => seq(
      'impl', $._type, $._type, '=', '{', repeat($.function_definition), '}',
    ),

    project_definition: $ => seq(
      'project', field('name', $.identifier), $.parameter_list,
      '->', 'inout', $._type, optional($.effect_row), '=', $._body,
      optional(';'),
    ),

    effect_row: $ => seq('<', sep1('|', $._type), '>'),

    member: $ => seq('let', $.identifier, $.parameter_list, '->', $._type, ';'),

    parameter_list: $ => seq('(', sep(',', optional($.parameter)), ')'),

    parameter: $ => seq(optional($.convention), $.identifier, ':', $._type),

    convention: $ => choice('inout', 'sink', 'set', 'let'),

    _type: $ => choice(
      $.path,
      $.type_application,
      $.raw_pointer,
      $.tuple_type,
      $.array_type,
      $.function_type,
      $.dyn_type,
    ),

    dyn_type: $ => seq('dyn', $.path),
    raw_pointer: $ => seq('&raw', optional('mut'), $._type),
    type_application: $ => prec.left(1, seq($.identifier, repeat1($._type))),
    tuple_type: $ => seq('(', sep(',', $._type), ')'),
    array_type: $ => seq('[', $._type, ';', $.number, ']'),
    function_type: $ => seq('(', sep(',', $._type), ')', '->', $._type),

    _body: $ => choice($.block, $._expression),
    // A block is a sequence of statements; an expression in tail position is an
    // expression *statement*, which is what removes the block/statement ambiguity.
    block: $ => seq('{', repeat($._statement), '}'),

    statement: $ => seq($._statement_kind, optional(';')),
    _statement_kind: $ => choice(
      $.variable_declaration,
      $.set_statement,
      $.yield_statement,
      $.when_statement,
      $.for_statement,
      $._expression,
      $.let_stmt,
    ),
    _statement: $ => $._statement_kind,
    expression_statement: $ => seq($._expression, optional(';')),

    let_stmt: $ => seq('let', $.identifier, $._expression),
    variable_declaration: $ => choice(
      seq('var', $.identifier, optional(seq(':', $._type)), '=', $._expression),
      seq($.identifier, ':=', $._expression),
    ),
    set_statement: $ => seq('set', $._expression, '=', $._expression),
    yield_statement: $ => choice(seq('yield', 'inout', $._expression), seq('yield', $._expression)),
    when_statement: $ => seq('when', '(', $._expression, ')'),
    for_statement: $ => seq('for', '(', $.identifier, ':=', $._expression, ')', $._expression),

    // Operators take an *operand* - everything that is not itself an operator
    // application - which is what keeps the expression grammar unambiguous without
    // a precedence cascade.
    _operand: $ => choice(
      $.conditional,
      $.match_expression,
      $.lambda,
      $.handler,
      $.call_expression,
      $.index_expression,
      $.field_expression,
      $.try_expression,
      $.path,
      $.number,
      $.string,
      $.character,
      $.boolean,
      $.tuple_expression,
      $.array_expression,
      $.record_expression,
      $.block,
    ),

    _expression: $ => choice(
      $.conditional,
      $.match_expression,
      $.lambda,
      $.handler,
      $.binary_expression,
      $.unary_expression,
      $.call_expression,
      $.index_expression,
      $.application,
      $.field_expression,
      $.path,
      $.number,
      $.string,
      $.character,
      $.boolean,
      $.tuple_expression,
      $.array_expression,
      $.record_expression,
      $.block,
    ),

    conditional: $ => seq('if', '(', $._expression, ')', $._body, optional(seq('else', $._body))),
    match_expression: $ => seq('match', $._expression, '{', repeat(seq($._expression, '->', $._expression, ',')), '}'),
    lambda: $ => seq('{', repeat(seq($.identifier, ',')), optional($.identifier), '->', $._expression, '}'),
    handler: $ => seq(choice('option', 'one', 'all', 'sync', 'race', 'rush', 'branch', 'comptime'), '{', repeat($.statement), '}'),
    record_expression: $ => seq($.identifier, '{', sep(',', seq($.identifier, ':', $._expression)), '}'),

    binary_expression: $ => prec.left(1, seq($._expression, $.operator, $._expression)),
    unary_expression: $ => prec(2, seq(choice('not', '-', '&raw', 'inout', 'unsafe'), $._operand)),
    call_expression: $ => prec(4, seq($._operand, '(', sep(',', $._expression), ')')),
    index_expression: $ => prec(4, seq($._operand, '[', sep(',', $._expression), ']')),
    application: $ => prec(3, seq($._operand, $._operand)),
    field_expression: $ => prec(5, seq($._operand, '.', choice($.identifier, $.number))),
    try_expression: $ => prec(6, seq($._expression, '?')),

    operator: $ => choice(
      '+', '-', '*', '/', '%', '++', '==', '!=', '<', '>', '<=', '>=',
      'and', 'or', '|', '=', ':=', '->', '&', '^', '<<', '>>',
    ),

    path: $ => prec(5, seq($.identifier, repeat(seq('::', $.identifier)))),
    // `()` is the empty tuple, so there is no separate unit rule to conflict with.
    tuple_expression: $ => seq('(', optional(sep(',', $._expression)), ')'),
    array_expression: $ => seq('[', sep(',', $._expression), ']'),

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,
    number: $ => /[0-9]+(\.[0-9]+)?([a-z0-9]+)?/,
    string: $ => choice($.raw_string, $.text),
    text: $ => /"([^"\\]|\\.)*"/,
    raw_string: $ => /r#"([^"]|"[^#])*"#/,
    character: $ => /'([^'\\]|\\.)'/,
    boolean: $ => choice('true', 'false'),

    line_comment: $ => token(choice(seq('//', /[^\n]*/), seq('#', /[^\n]*/))),
    block_comment: $ => token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),
  },
});

function sep(separator, rule) {
  return optional(seq(rule, repeat(seq(separator, rule))));
}

function sep1(separator, rule) {
  return seq(rule, repeat(seq(separator, rule)));
}
