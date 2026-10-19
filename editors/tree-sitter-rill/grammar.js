// A grammar for Rill, for editor highlighting and structural selection rather
// than for parsing semantics: the compiler's parser (crates/rill-parser) stays
// the reference. The shape here follows it closely:
//
//   * statements are `kind ";"`; only a block's or module's *tail* expression
//     may omit the `;`, exactly like the reference parser's
//     `semi_or_block_end`;
//   * `=` is the equality operator, so `set place = expr` parses its place at
//     postfix precedence and stops before the store `=`;
//   * `.` is a tight postfix projection (`from.balance`, `part.0`,
//     `acc . balance_mut`);
//   * `f x y` is an application, `f(x, y)` / `f[x, y]` are calls, and
//     `name { ... }` is the handler/builder application form (`handler`);
//   * `as`/`is` cast to a type, and every operator has the binding power of
//     `crates/rill-syntax`'s table.
module.exports = grammar({
  name: 'rill',

  extras: $ => [/\s/, $.line_comment, $.block_comment],

  word: $ => $.identifier,

  rules: {
    // A module is declarations and statements; like a block, it may end with a
    // bare tail expression (the reference parser leaves it unwrapped).
    source_file: $ => seq(
      repeat($.use_declaration),
      repeat($._item),
      optional($._statement_kind),
    ),

    // `use path ;`, `use path::* ;`, `use path::{a, b} ;`, `use "text" ;`
    use_declaration: $ => seq(
      'use',
      choice(
        $.text,
        $.use_path,
      ),
      ';',
    ),

    // `a::b::c`, `a::{b, c}`, `a::*`
    use_path: $ => prec.right(seq(
      $.identifier,
      repeat(seq('::', $.identifier)),
      optional($.use_tail),
    )),

    use_tail: $ => choice(
      seq('::', $.use_group),
      seq('::', '*'),
    ),

    use_group: $ => seq('{', optional(sep(',', $.identifier)), '}'),

    // `@name ("text")? ;` — a module-level directive (`@include("...")`).
    attribute_declaration: $ => seq(
      '@', field('name', $.identifier),
      optional(seq('(', optional($.text), ')')),
      ';',
    ),

    _item: $ => choice(
      $.function_definition,
      $.type_definition,
      $.trait_definition,
      $.impl_definition,
      $.project_definition,
      $.extern_declaration,
      $.builder_definition,
      $.attribute_declaration,
      $.statement,
    ),

    // `let name params ("->" convention? type effects?)? ("=" body)? ;?`
    //
    // The reference parser accepts a body-less signature (`extern`) and a
    // parameterless binding (`let x = e`), so the tail after the name is a
    // choice rather than a single optional chain — which also keeps the parse
    // unambiguous when a bare `let name` would otherwise complete early.
    function_definition: $ => seq(
      'let', field('name', $.identifier),
      choice(
        seq($.parameter_list, optional($._result_type),
          optional(seq('=', $._expression))),
        seq('->', optional($.convention), $._type, optional($.effect_row),
          optional(seq('=', $._expression))),
        seq('=', $._expression),
      ),
      optional(';'),
    ),

    _result_type: $ => seq(
      '->', optional($.convention), $._type, optional($.effect_row),
    ),

    // `extern "c"? (let ... | name: type ;)` — an FFI binding.
    extern_declaration: $ => seq(
      'extern', optional($.text),
      choice($.function_definition, $.bare_signature),
    ),

    // `name ":" type effects? ;` — a bare signature (trait and FFI members).
    bare_signature: $ => seq(
      field('name', $.identifier), ':', $._type, optional($.effect_row), ';',
    ),

    // `project name params -> inout type effects? = body ;?`
    project_definition: $ => seq(
      'project', field('name', $.identifier), $.parameter_list,
      '->', optional($.convention), $._type, optional($.effect_row),
      '=', $._expression,
      optional(';'),
    ),

    // `type Name type_params? = body ;`
    type_definition: $ => seq(
      'type', field('name', $.identifier), repeat($.type_parameter),
      '=', $.type_body, ';',
    ),

    // A type parameter is a bare name or `(comptime n : type)`.
    type_parameter: $ => choice(
      $.identifier,
      seq('(', 'comptime', field('name', $.identifier), ':', $._type, ')'),
    ),

    // A type body is a record, an algebraic variant list, or an alias.
    type_body: $ => choice($.record_body, $.view_body, $.variant_list, $._type),

    record_body: $ => seq('record', '{', repeat(choice($.member, $.deinit_clause)), '}'),

    // `view { ... }` — the same fields, without ownership.
    view_body: $ => seq('view', '{', repeat($.member), '}'),

    // `deinit (sink name) block` — a record's destructor.
    deinit_clause: $ => seq(
      'deinit', '(', optional($.convention), field('name', $.identifier), ')', $.block,
    ),

    // `var? name ":" type ";"` — a record field. The `;` is optional before `}`.
    member: $ => seq(
      optional('var'), field('name', $.identifier), ':', $._type, optional(';'),
    ),

    // `|? Variant ("|" Variant)*`, `Variant = Name type_atom*`. A single bare
    // name stays a type alias (the reference parser's `variants_ahead` needs a
    // `|`), so the leading-`|` form and the `A | B` form are both listed.
    variant_list: $ => choice(
      seq('|', sep1('|', $.variant)),
      seq($.variant, '|', repeat1(seq('|', $.variant))),
    ),

    variant: $ => seq(field('name', $.identifier), repeat($._type_atom)),

    // `trait Name type_params? where? =? { signature* } ;?`
    trait_definition: $ => seq(
      'trait', choice($.type_application, field('name', $.identifier)),
      optional($.constraint_list), optional('='),
      '{', repeat(choice($.function_signature, $.bare_signature)), '}', optional(';'),
    ),

    // `let name params -> type effects? ;` — a trait member.
    function_signature: $ => seq(
      'let', field('name', $.identifier), $.parameter_list, $._result_type, ';',
    ),

    // `impl Trait Type = { definition* } ;?` — the head is a name applied to
    // type arguments, the same shape as `Array i32` in the type grammar.
    impl_definition: $ => seq(
      'impl', choice($.type_application, field('name', $.identifier)),
      optional($.constraint_list), '=',
      '{', repeat(choice($.function_definition, $.bare_signature)), '}', optional(';'),
    ),

    // `builder Name type_params? = { members } ;`
    builder_definition: $ => seq(
      'builder', field('name', $.identifier), repeat($.type_parameter), '=',
      '{', repeat(choice($.function_definition, $.bare_signature)), '}', ';',
    ),

    constraint_list: $ => seq('where', sep1('|', $._constraint)),

    _constraint: $ => seq($._type, 'is', $._type_atom),

    // `<effect ("|" effect)*>`, `effect = name type_atom*`.
    effect_row: $ => seq('<', sep1('|', $._effect_item), '>'),

    _effect_item: $ => seq($.identifier, repeat($._type_atom)),

    parameter_list: $ => seq('(', optional(sep(',', $.parameter)), ')'),

    // `convention? name ":" convention? type` — the convention may sit on
    // either side of the `:` (`from: inout Account`).
    parameter: $ => seq(
      optional($.convention), field('name', $.identifier), ':',
      optional($.convention), $._type,
    ),

    convention: $ => choice('inout', 'sink', 'set', 'let'),

    // ---- types ----------------------------------------------------------

    _type: $ => choice(
      $.type_path,
      $.type_application,
      $.raw_pointer,
      $.tuple_type,
      $.array_type,
      $.function_type,
      $.dyn_type,
    ),

    // An atom is a type that cannot itself be an application.
    _type_atom: $ => choice(
      $.type_path,
      $.raw_pointer,
      $.tuple_type,
      $.array_type,
      $.dyn_type,
    ),

    type_path: $ => seq($.identifier, repeat(seq('::', $.identifier))),

    // `Array i32`, `State s`, `impl Show Point` — a constructor applied to
    // type atoms.
    type_application: $ => prec.left(1, seq(
      field('name', $.identifier), repeat1($._type_atom),
    )),

    // `&raw T` / `&raw mut T`.
    raw_pointer: $ => prec(2, seq('&', 'raw', optional('mut'), $._type_atom)),

    // `()`, `(T)`, `(T, U)` — grouping and tuples share one node.
    tuple_type: $ => seq('(', optional(sep(',', $._type)), ')'),

    array_type: $ => seq('[', $._type, ';', $._expression, ']'),

    // `(A, B) -> C <effects>` — effect rows bind to function types.
    function_type: $ => prec.right(3, seq(
      $.tuple_type, '->', optional($.convention), $._type, optional($.effect_row),
    )),

    dyn_type: $ => seq('dyn', $._type_atom),

    // ---- statements and blocks ------------------------------------------

    // Statements require `;`; only the block's tail expression may omit it,
    // which is exactly the reference parser's `semi_or_block_end` rule. A
    // `let` declaration is a block item, not a statement (its `;` is optional).
    block: $ => seq(
      '{', repeat(choice($.statement, $.function_definition)),
      optional($._statement_kind), '}',
    ),

    statement: $ => seq($._statement_kind, ';'),

    _statement_kind: $ => choice(
      $.variable_declaration,
      $.set_statement,
      $.yield_statement,
      $.when_statement,
      $.for_statement,
      $.while_statement,
      $.break_statement,
      $.let_stmt,
      $._expression,
    ),

    // `static threadlocal? var name (: type)? = expr`, `var …`, or `name := expr`.
    variable_declaration: $ => choice(
      seq('static', optional('threadlocal'), 'var', field('name', $.identifier),
        optional(seq(':', $._type)), '=', $._expression),
      seq('var', field('name', $.identifier), optional(seq(':', $._type)),
        '=', $._expression),
      seq(field('name', $.identifier), ':=', $._expression),
    ),

    // `set place = expr` — the place is postfix-only, so `=` stays the store.
    set_statement: $ => seq('set', $._postfix, '=', $._expression),

    // `yield inout? expr`
    yield_statement: $ => seq('yield', optional('inout'), $._expression),

    // `when (cond)` — a handler guard.
    when_statement: $ => seq('when', '(', $._condition, ')'),

    // `for (name := iter) body` (the reference parser also accepts `:`).
    for_statement: $ => prec(1, seq(
      'for', '(', field('name', $.identifier), choice(':=', ':'),
      $._expression, ')',
      choice($.block, $.statement),
    )),

    while_statement: $ => prec(1, seq(
      'while', '(', $._expression, ')',
      choice($.block, $.statement),
    )),

    // `break` — a bare keyword; the enclosing `statement` supplies the `;`.
    break_statement: $ => 'break',

    // `let? name = expr` — the failable binding of PLAN §6.1.
    let_stmt: $ => seq('let', '?', field('name', $.identifier), '=', $._expression),

    // ---- expressions ----------------------------------------------------

    _expression: $ => choice(
      $._postfix,
      $.binary_expression,
      $.cast_expression,
    ),

    _postfix: $ => choice(
      $._primary,
      $.call_expression,
      $.index_expression,
      $.field_expression,
      $.try_expression,
      $.application,
    ),

    _primary: $ => choice(
      $._atom,
      $.block,
      $.lambda,
    ),

    // Everything that may stand alone or as an operand of an application. A
    // bare block/lambda is *not* one: `name { ... }` is the handler form and
    // `match x { ... }` owns its brace.
    _atom: $ => choice(
      $.path,
      $.number,
      $.string,
      $.character,
      $.boolean,
      $.tuple_expression,
      $.array_expression,
      $.conditional,
      $.match_expression,
      $.handler,
      $.unary_expression,
      $.comptime_expression,
      $.unsafe_expression,
    ),

    // `f x y` — space-separated application (left associative).
    application: $ => prec.left(13, seq($._postfix, $._atom)),

    // `f(a, b)` / `f (a, b)` — the reference parser splices a spaced argument
    // list into the call, so adjacency is not observable here.
    call_expression: $ => prec.left(14, seq($._postfix, '(', sep(',', $._argument), ')')),

    // `e[i]` / `e[i, j]` — index or adjacent (fail-fast) call. The bracket
    // group is the array of arguments, so it is an `array_expression`.
    index_expression: $ => prec.left(14, seq($._postfix, $.array_expression)),

    // `e.name`, `e.0`, `e . projection` — the flow/projection dot.
    field_expression: $ => prec.left(15, seq($._postfix, '.', choice($.identifier, $.number))),

    // `e?`
    try_expression: $ => prec(14, seq($._postfix, '?')),

    // Parenthesised argument lists win over reading `(` as a tuple.
    _argument: $ => prec(1, seq(optional($.convention), $._expression)),

    // `if (cond) block (else (if ... | block))?`; the condition may also be a
    // failable binding, `if (x := f[y])`.
    conditional: $ => prec.right(seq(
      'if', '(', $._condition, ')', $.block,
      optional(seq('else', choice($.conditional, $.block))),
    )),

    _condition: $ => choice($._binding, $._expression),

    // `name := expr` in a condition position.
    _binding: $ => seq(field('name', $.identifier), ':=', $._expression),

    // `match e { (|)? pattern (if guard)? -> e (,|;)? ... }`
    match_expression: $ => seq(
      'match', $._expression, '{', repeat($.match_arm), '}',
    ),

    match_arm: $ => seq(
      optional('|'),
      $.pattern,
      optional(seq('if', $._expression)),
      '->', $._expression,
      optional(choice(',', ';')),
    ),

    pattern: $ => choice(
      seq($.path, repeat($._pattern_atom)),
      $.number,
      $.string,
      $.character,
      $.boolean,
      $.tuple_pattern,
    ),

    _pattern_atom: $ => choice(
      $.identifier,
      $.number,
      $.string,
      $.character,
      $.boolean,
      $.tuple_pattern,
    ),

    tuple_pattern: $ => seq('(', optional(sep(',', $.pattern)), ')'),

    // `{ x, y -> e }` / `{ -> e }`
    lambda: $ => seq(
      '{', optional(sep(',', $.identifier)), '->', $._expression, '}',
    ),

    // `option { ... }`, `all { ... }`, `sync { ... }` … — the handler and
    // builder forms: a name applied to a block argument. The reference parser
    // models these as a call whose argument is the block, so any name is
    // allowed here and the named node keeps the common ones highlightable.
    handler: $ => prec.dynamic(-1, seq(field('name', $.identifier), $.block)),

    comptime_expression: $ => seq('comptime', $.block),
    unsafe_expression: $ => seq('unsafe', $.block),

    // Prefix operators; tighter than every binary operator and the cast.
    unary_expression: $ => prec(12, seq(
      choice('not', '-', 'size', 'shapeof', 'reshape'), $._postfix,
    )),

    // `e as T` / `e is T` — the right side is a type.
    cast_expression: $ => prec.left(10, seq($._expression, choice('as', 'is'), $._type)),

    // Binding powers follow crates/rill-syntax/src/lib.rs `bp`; the operator
    // itself is a named node so queries can capture every operator at once.
    binary_expression: $ => choice(
      // choice / fallback: `a | b`
      prec.left(1, seq($._expression, field('operator', alias($._op_choice, $.operator)), $._expression)),
      // ranges
      prec.left(2, seq($._expression, field('operator', alias($._op_range, $.operator)), $._expression)),
      // `or` / `and`
      prec.left(3, seq($._expression, field('operator', alias($._op_or, $.operator)), $._expression)),
      prec.left(4, seq($._expression, field('operator', alias($._op_and, $.operator)), $._expression)),
      // equality (non-associative in the reference table)
      prec.left(5, seq($._expression, field('operator', alias($._op_equality, $.operator)), $._expression)),
      // comparison
      prec.left(6, seq($._expression, field('operator', alias($._op_compare, $.operator)), $._expression)),
      // concatenation
      prec.right(7, seq($._expression, field('operator', alias($._op_concat, $.operator)), $._expression)),
      // additive
      prec.left(8, seq($._expression, field('operator', alias($._op_additive, $.operator)), $._expression)),
      // multiplicative
      prec.left(9, seq($._expression, field('operator', alias($._op_multiplicative, $.operator)), $._expression)),
    ),

    _op_choice: $ => '|',
    _op_range: $ => choice('..', '..<'),
    _op_or: $ => 'or',
    _op_and: $ => 'and',
    _op_equality: $ => choice('=', '==', '!='),
    _op_compare: $ => choice('<', '>', '<=', '>='),
    _op_concat: $ => '++',
    _op_additive: $ => choice('+', '-', '^', '|&'),
    _op_multiplicative: $ => choice('*', '/', '%', '<<', '>>', '&'),

    // Every binary operator, with the precedence that governs `.`-style flow:
    // `set` and `binary_expression` both range over this table.
    operator: $ => choice(
      '|', '..', '..<', 'or', 'and', '=', '==', '!=', '<', '>', '<=', '>=',
      '++', '+', '-', '^', '|&', '*', '/', '%', '<<', '>>', '&',
    ),

    // `name ("::" name)*`
    path: $ => prec(5, seq($.identifier, repeat(seq('::', $.identifier)))),

    // `()` (unit), `(e)`, `(a, b)`
    tuple_expression: $ => seq('(', optional(sep(',', $._expression)), ')'),

    array_expression: $ => seq('[', optional(sep(',', $._argument)), ']'),

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,
    number: $ => /[0-9]+(\.[0-9]+)?([a-z][a-z0-9]*)?/,
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
