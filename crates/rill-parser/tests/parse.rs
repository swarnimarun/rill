//! Parser tests: lossless round-trip, the §2.5 adjacency rule, precedence,
//! and resilience on malformed input.

use rill_diagnostics::render_all;
use rill_parser::{parse_text, Parse};
use rill_span::SourceMap;
use rill_syntax::{SyntaxKind as K, SyntaxNode};

fn parse_ok(src: &str) -> (Parse, SourceMap) {
    let (parse, sm) = parse_text("test.rill", src);
    assert!(
        !parse.diagnostics.has_errors(),
        "unexpected errors for `{src}`:\n{}",
        render_all(&sm, &parse.diagnostics)
    );
    assert_eq!(parse.cst.text(&sm), src, "CST must round-trip exactly");
    (parse, sm)
}

fn root(parse: &Parse) -> &SyntaxNode {
    &parse.cst.root
}

#[test]
fn round_trip_handles_trivia_and_comments() {
    let src = "# leading\nlet x = 1; // trailing\n/* block /* nested */ */\n";
    let (parse, _) = parse_ok(src);
    assert_eq!(root(&parse).count(K::LetDecl), 1);
    // Tokens tile the file exactly, in order.
    let tokens = parse.cst.tokens();
    for pair in tokens.windows(2) {
        assert_eq!(pair[0].span.end, pair[1].span.start);
    }
}

#[test]
fn adjacency_rule_brackets() {
    // `xs[i]` is an index; `xs [i]` applies `xs` to an array literal.
    let (parse, _) = parse_ok("let a = xs[0];\nlet b = xs [0];\n");
    assert_eq!(root(&parse).count(K::IndexExpr), 1);
    let items = root(&parse).child_list();
    let b = items[1];
    let call = b.find(K::CallExpr).expect("application");
    assert!(
        call.find(K::ArrayExpr).is_some(),
        "spaced `[` is an array literal"
    );
    assert!(call.find(K::IndexExpr).is_none());
}

#[test]
fn adjacency_rule_calls_and_try() {
    let (parse, _) = parse_ok("let a = add(1, 2);\nlet b = xs?;\n");
    let items = root(&parse).child_list();
    let args = items[0].find(K::ArgList).expect("arg list");
    assert_eq!(args.count(K::Arg), 2);
    assert_eq!(items[1].count(K::TryExpr), 1);
    // Spaced parens are still a call: `rotate_point (inout a, b)`.
    let (parse, _) = parse_ok("rotate_point (inout t.position, t.rotation);");
    let call = root(&parse).find(K::CallExpr).expect("call");
    assert_eq!(call.count(K::Arg), 2);
    assert_eq!(
        call.count_tokens(K::KwInout),
        1,
        "first argument carries `inout`"
    );
}

#[test]
fn flow_operator_takes_the_application_spine() {
    // `x . f y` groups as `x . (f y)`.
    let (parse, sm) = parse_ok("let r = 10 . add 20;\n");
    let bin = root(&parse).find(K::BinaryExpr).expect("flow");
    assert!(bin.has_token(K::Dot));
    let call = bin.find(K::CallExpr).expect("rhs application");
    let callee = call.child_list()[0];
    assert_eq!(callee.text(&sm), "add");
    assert_eq!(call.count(K::Arg), 1, "`20` is the flowed argument");
}

#[test]
fn flow_binds_tighter_than_arithmetic() {
    // `from.balance - amount` is `(from.balance) - amount`, and
    // `alice.balance as String` is `(alice.balance) as String`.
    let (parse, _) = parse_ok("let f (a: Account, b: i64) = a.balance - b;\n");
    let root = root(&parse);
    let sub = root.find(K::BinaryExpr).expect("subtraction");
    assert!(sub.has_token(K::Minus), "the outermost operator is `-`");
    let flow = sub.child_list()[0];
    assert!(
        flow.has_token(K::Dot),
        "its left operand is the field access: {flow:?}"
    );
    assert_eq!(root.count(K::BinaryExpr), 2, "field access + subtraction");
}

#[test]
fn precedence_follows_the_table() {
    let (parse, _) = parse_ok("let r = a + b * c == d;\n");
    let ops = root(&parse).descendants(K::BinaryExpr);
    assert_eq!(ops.len(), 3, "==( +(a, *(b, c)), d )");
    assert!(ops[0].has_token(K::EqEq), "equality is loosest here");
    assert!(ops[1].has_token(K::Plus));
    assert!(ops[2].has_token(K::Star));

    // `++` is right-associative.
    let (parse, _) = parse_ok("let s = a ++ b ++ c;\n");
    let outer = root(&parse).find(K::BinaryExpr).unwrap();
    let rhs = outer.child_list()[1];
    assert_eq!(rhs.kind, K::BinaryExpr, "right operand is itself a `++`");
}

#[test]
fn statements_versus_tail_expression() {
    let (parse, _) = parse_ok("let f () = {\n    x := 1;\n    y := 2;\n    x + y\n};\n");
    let block = root(&parse).find(K::Block).expect("block");
    let stmts: Vec<K> = block.child_list().iter().map(|n| n.kind).collect();
    assert_eq!(
        stmts,
        vec![K::BindStmt, K::BindStmt, K::BinaryExpr],
        "last is a tail expression"
    );
}

#[test]
fn lambda_versus_block() {
    let (parse, _) = parse_ok("let r = fold 0 { acc, x -> acc + x };\n");
    let lambda = root(&parse).find(K::LambdaExpr).expect("lambda");
    let params = lambda.child_node(K::LambdaParams).expect("parameters");
    assert_eq!(params.count_tokens(K::Name), 2);
    // A block without `->` is not a lambda.
    let (parse, _) = parse_ok("let r = f { x };\n");
    assert!(root(&parse).find(K::LambdaExpr).is_none());
    assert!(root(&parse).find(K::Block).is_some());
}

#[test]
fn declaration_shapes() {
    let (parse, _) = parse_ok(
        "use core::alloc::StackArena;\n\
         type Account = record { id: u64; var balance: i64; deinit (sink a) { } };\n\
         type MatrixView a = view { data: Slice a; rows: usize; };\n\
         project balance_mut (acc: inout Account) -> inout i64 <computes> = { yield inout acc.balance; };\n\
         let transfer (from: inout Account, to: inout Account, amount: i64) -> () <decides | transacts> = {\n\
             amount > 0;\n\
         };\n",
    );
    let root = root(&parse);
    assert_eq!(root.count(K::UseDecl), 1);
    assert_eq!(root.count(K::TypeDecl), 2);
    assert_eq!(root.count(K::RecordDef), 1);
    assert_eq!(root.count(K::FieldDef), 4);
    assert_eq!(root.count(K::DeinitClause), 1);
    assert_eq!(root.count(K::ProjectDecl), 1);
    assert_eq!(root.count(K::YieldStmt), 1);
    assert_eq!(root.count(K::LetDecl), 1);
    assert!(root.find(K::TypeView).is_some(), "view types parse");
    let rows: Vec<usize> = root
        .descendants(K::EffectRow)
        .iter()
        .map(|r| r.count(K::EffectItem))
        .collect();
    assert_eq!(rows, vec![1, 2], "<computes>, then <decides | transacts>");
}

#[test]
fn if_with_failable_binding_and_brace_forms() {
    let (parse, _) = parse_ok(
        "let f () = {\n\
             if (pair := xs . split_at[i]) { g (inout pair.0); } else { () }\n\
         };\n\
         _ := option { transfer[inout a, b, 200] };\n\
         _ := all { x := 1..5; y := 1..x; when (x + y = 5); (x, y) };\n",
    );
    let root = root(&parse);
    let if_expr = root.find(K::IfExpr).expect("if expression");
    assert_eq!(if_expr.count(K::BindStmt), 1, "`if (x := ...)` binds");
    assert_eq!(if_expr.count(K::Block), 2, "then + else");
    assert_eq!(root.count(K::Block), 5, "f body, then, else, option, all");
    assert_eq!(root.count(K::WhenStmt), 1);
    assert_eq!(root.count(K::TupleExpr), 1, "the `all` tail is a tuple");
}

#[test]
fn for_loops_both_binding_forms() {
    let (parse, _) =
        parse_ok("let f () = {\n    for (i := 0..n) out . push i;\n    for (x : xs) { g x }\n};\n");
    assert_eq!(root(&parse).count(K::ForStmt), 2);
}

#[test]
fn malformed_input_recovers_and_still_round_trips() {
    let src = "let x = );\nlet y = 2;\n";
    let (parse, sm) = parse_text("bad.rill", src);
    assert!(parse.diagnostics.has_errors(), "must report the bad token");
    assert_eq!(parse.cst.text(&sm), src, "lossless even when broken");
    // Recovery still finds the second declaration.
    assert_eq!(parse.cst.root.count(K::LetDecl), 2);
    let rendered = render_all(&sm, &parse.diagnostics);
    assert!(rendered.contains("bad.rill:1:"), "{rendered}");
}

#[test]
fn module_level_signature_and_extern() {
    let (parse, _) = parse_ok(
        "count : a -> usize <computes>;\n\
         extern \"c\" let puts (s: &raw u8) -> i32 <io>;\n\
         builder seq = { let ret (u: ()) -> Step a <computes> = Step::Done; };\n",
    );
    let root = root(&parse);
    assert_eq!(root.count(K::SignatureDecl), 1);
    assert_eq!(root.count(K::ExternDecl), 1);
    assert_eq!(root.count(K::BuilderDecl), 1);
    assert_eq!(root.count(K::BuilderBody), 1);
    assert!(root.find(K::TypePtr).is_some(), "&raw u8");
}

#[test]
fn application_spine_is_flattened() {
    // §9: `add 10 20` groups as one call with two arguments, not a nested
    // `(add 10) 20`, so arity analysis sees the whole spine at once.
    let (parse, _) = parse_ok("let r = add 10 20;\n");
    let calls = root(&parse).descendants(K::CallExpr);
    assert_eq!(
        calls.len(),
        1,
        "a flat spine is a single CallExpr: {:?}",
        calls.len()
    );
    assert_eq!(calls[0].count(K::Arg), 2);

    // Explicit grouping stays visible: `(f x) y` has two call nodes.
    let (parse, _) = parse_ok("let r = (f x) y;\n");
    assert_eq!(root(&parse).descendants(K::CallExpr).len(), 2);

    // Argument lists splice: `g (a, b) c` is one call with three arguments.
    let (parse, _) = parse_ok("let r = g (a, b) c;\n");
    let calls = root(&parse).descendants(K::CallExpr);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].count(K::Arg), 3);
}

#[test]
fn match_arms_and_patterns() {
    let (parse, _) = parse_ok(
        "let classify (p: Pair) = match p {\n\
             | Pair (a, b) if a > b -> a,\n\
             | Pair (a, b) -> b,\n\
             _ -> 0\n\
         };\n",
    );
    let match_expr = root(&parse).find(K::MatchExpr).expect("match");
    assert_eq!(match_expr.count(K::MatchArm), 3);
    let arms: Vec<&SyntaxNode> = match_expr.descendants(K::MatchArm);
    assert!(arms[0].has_token(K::KwIf), "first arm has a guard");
    assert!(!arms[1].has_token(K::KwIf));
    let arm_pattern = arms[0].child_node(K::MatchPat).expect("arm pattern");
    let tuple_pattern = arm_pattern
        .child_node(K::MatchPat)
        .expect("nested tuple pattern");
    assert_eq!(
        tuple_pattern.child_list().len(),
        2,
        "`(a, b)` binds two names"
    );
    assert!(arms[2].has_descendant_token(K::Underscore), "wildcard arm");
}

#[test]
fn contextual_operators_are_not_application_arguments() {
    let (parse, _) = parse_ok("let r = a and b;\n");
    let ops = root(&parse).descendants(K::BinaryExpr);
    assert_eq!(ops.len(), 1, "`a and b` is one binary expression");
    assert_eq!(
        root(&parse).count(K::CallExpr),
        0,
        "not an application spine"
    );

    // A binding never becomes an argument of the preceding expression, so a
    // `;`-less block-like statement can be followed by a bind.
    let (parse, _) = parse_ok("let f (c: Bool) = {\n    if (c) { g x }\n    y := 1;\n};\n");
    let block = root(&parse).find(K::Block).expect("block");
    let kinds: Vec<K> = block.child_list().iter().map(|n| n.kind).collect();
    assert_eq!(kinds, vec![K::ExprStmt, K::BindStmt], "{kinds:?}");
    assert!(
        block.child_list()[0].find(K::IfExpr).is_some(),
        "the `;`-less statement still parses"
    );
}

#[test]
fn attributes_may_precede_an_item() {
    let (parse, sm) = parse_ok("@link(\"SDL3\");\n@inline;\nuse m;\n");
    let root = root(&parse);
    let items = root.child_list();
    assert_eq!(items[0].kind, K::AttributeDecl);
    assert_eq!(items[1].kind, K::AttributeDecl);
    assert_eq!(items[2].kind, K::UseDecl);
    assert!(items[0].has_token(K::At), "@ is in the CST");
    assert_eq!(items[0].text(&sm), "@link(\"SDL3\");");
    assert_eq!(items[1].text(&sm), "@inline;", "value is optional");
    assert_eq!(root.count_tokens(K::At), 2);
}

#[test]
fn include_in_a_type_body() {
    let (parse, sm) = parse_ok("type M = @include(\"SDL3/SDL.h\");\n");
    let root = root(&parse);
    let decl = root.find(K::TypeDecl).expect("type declaration");
    let include = decl.find(K::Include).expect("include body");
    assert!(include.has_token(K::At));
    assert!(include.has_token(K::Text));
    assert_eq!(include.text(&sm), "@include(\"SDL3/SDL.h\")");
}

#[test]
fn use_plain_glob_and_group_paths() {
    let (parse, _) = parse_ok("use M;\nuse A::B::*;\nuse core::alloc::{X, Y};\n");
    let root = root(&parse);
    let uses = root.descendants(K::UsePath);
    assert_eq!(uses.len(), 3, "one UsePath per non-text use");
    assert_eq!(uses[0].count_tokens(K::Star), 0);
    assert_eq!(uses[1].count_tokens(K::Star), 1);
    assert!(uses[1].has_token(K::ColonColon), "`::*` keeps its separator");
    assert_eq!(root.count(K::UseGroup), 1);
}

#[test]
fn keyword_after_dot_is_a_field_path() {
    let (parse, sm) = parse_ok("let f (ev: Event) = ev.type;\nlet g (x: X) = x.view;\nlet h (p: P) = p.match;\n");
    let root = root(&parse);
    let flows = root.descendants(K::BinaryExpr);
    assert_eq!(flows.len(), 3);
    for (flow, text) in flows.iter().zip(["type", "view", "match"]) {
        assert!(flow.has_token(K::Dot));
        let rhs = &flow.child_list()[1];
        assert_eq!(rhs.kind, K::PathExpr, "keyword is a plain path");
        assert_eq!(rhs.text(&sm), text);
    }
}
