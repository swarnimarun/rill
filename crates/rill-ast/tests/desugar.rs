//! Builder-desugaring tests (PLAN.md §6.1, §3.3, §5.3, §7.2).
//!
//! Assertions are made against a compact S-expression rendering of the
//! desugared AST so the expected shapes stay readable.

use rill_ast::{
    parse_and_lower_text, Arg, Block, ConcurrentKind, Cond, ElseBranch, Expr, Frontend,
    HandlerKind, Item, Stmt, UnaryOp,
};

fn frontend(src: &str) -> (Frontend, rill_span::SourceMap) {
    let (frontend, sm) = parse_and_lower_text("desugar.rill", src);
    assert!(
        !frontend.diagnostics.has_errors(),
        "unexpected diagnostics:\n{}",
        rill_diagnostics::render_all(&sm, &frontend.diagnostics)
    );
    (frontend, sm)
}

/// Body of the first `let`/`project` item named `name`.
fn body_of(frontend: &Frontend, name: &str) -> Expr {
    for item in &frontend.module.items {
        if let Item::Fn(decl) = item {
            if frontend.text(decl.name) == name {
                return decl.body.clone().expect("a body");
            }
        }
    }
    panic!("no item named `{name}`");
}

fn dump(frontend: &Frontend, expr: &Expr) -> String {
    let mut out = String::new();
    write_expr(frontend, expr, &mut out);
    out
}

fn write_expr(frontend: &Frontend, expr: &Expr, out: &mut String) {
    match expr {
        Expr::Path { segments, .. } => {
            let parts: Vec<&str> = segments.iter().map(|s| frontend.text(*s)).collect();
            out.push_str(&parts.join("::"));
        }
        Expr::Literal { text, .. } => out.push_str(text),
        Expr::Tuple { elems, .. } => {
            if elems.is_empty() {
                out.push_str("unit");
            } else {
                out.push_str("(tuple");
                for e in elems {
                    out.push(' ');
                    write_expr(frontend, e, out);
                }
                out.push(')');
            }
        }
        Expr::Array { elems, .. } => {
            out.push_str("(array");
            for e in elems {
                out.push(' ');
                write_expr(frontend, e, out);
            }
            out.push(')');
        }
        Expr::Call { callee, args, .. } => {
            out.push('(');
            write_expr(frontend, callee, out);
            write_args(frontend, args, out);
            out.push(')');
        }
        Expr::Index { base, args, .. } => {
            out.push_str("(index ");
            write_expr(frontend, base, out);
            write_args(frontend, args, out);
            out.push(')');
        }
        Expr::Try { base, .. } => {
            out.push_str("(try ");
            write_expr(frontend, base, out);
            out.push(')');
        }
        Expr::Unary { op, operand, .. } => {
            let name = match op {
                UnaryOp::Neg => "neg",
                UnaryOp::Not => "not",
                UnaryOp::Deref => "deref",
                UnaryOp::Size => "size",
                UnaryOp::Shapeof => "shapeof",
                UnaryOp::Reshape => "reshape",
            };
            out.push('(');
            out.push_str(name);
            out.push(' ');
            write_expr(frontend, operand, out);
            out.push(')');
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            out.push('(');
            out.push_str(&format!("{op:?}"));
            out.push(' ');
            write_expr(frontend, lhs, out);
            out.push(' ');
            write_expr(frontend, rhs, out);
            out.push(')');
        }
        Expr::Cast { expr, ty, .. } => {
            out.push_str("(cast ");
            write_expr(frontend, expr, out);
            out.push(' ');
            write_ty(frontend, ty, out);
            out.push(')');
        }
        Expr::Is { expr, ty, .. } => {
            out.push_str("(is ");
            write_expr(frontend, expr, out);
            out.push(' ');
            write_ty(frontend, ty, out);
            out.push(')');
        }
        Expr::Block(block) => write_block(frontend, block, out),
        Expr::Lambda { params, body, .. } => {
            out.push_str("(lambda (");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&pattern_text(frontend, p));
            }
            out.push_str(") ");
            write_block(frontend, body, out);
            out.push(')');
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            out.push_str("(if ");
            write_cond(frontend, cond, out);
            out.push(' ');
            write_block(frontend, then_branch, out);
            match else_branch {
                Some(ElseBranch::Block(block)) => {
                    out.push(' ');
                    write_block(frontend, block, out);
                }
                Some(ElseBranch::If(e)) => {
                    out.push(' ');
                    write_expr(frontend, e, out);
                }
                None => {}
            }
            out.push(')');
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            out.push_str("(match ");
            write_expr(frontend, scrutinee, out);
            for arm in arms {
                out.push_str(" (arm ");
                out.push_str(&pattern_text(frontend, &arm.pattern));
                if let Some(guard) = &arm.guard {
                    out.push_str(" (guard ");
                    write_expr(frontend, guard, out);
                    out.push(')');
                }
                out.push(' ');
                write_expr(frontend, &arm.body, out);
                out.push(')');
            }
            out.push(')');
        }
        Expr::Comptime { body, .. } => {
            out.push_str("(comptime ");
            write_block(frontend, body, out);
            out.push(')');
        }
        Expr::Unsafe { body, .. } => {
            out.push_str("(unsafe ");
            write_block(frontend, body, out);
            out.push(')');
        }
        Expr::Handler { kind, body, .. } => {
            let name = match kind {
                HandlerKind::Option => "option",
                HandlerKind::One => "one",
                HandlerKind::All => "all",
            };
            out.push_str("(handler ");
            out.push_str(name);
            out.push(' ');
            write_block(frontend, body, out);
            out.push(')');
        }
        Expr::Concurrent { kind, arms, .. } => {
            let name = match kind {
                ConcurrentKind::Sync => "sync",
                ConcurrentKind::Race => "race",
                ConcurrentKind::Rush => "rush",
                ConcurrentKind::Branch => "branch",
            };
            out.push_str("(concurrent ");
            out.push_str(name);
            for arm in arms {
                out.push(' ');
                write_expr(frontend, arm, out);
            }
            out.push(')');
        }
        Expr::Error(_) => out.push_str("<error>"),
    }
}

fn write_args(frontend: &Frontend, args: &[Arg], out: &mut String) {
    for arg in args {
        out.push(' ');
        if let Some(conv) = arg.convention {
            out.push_str(&format!("{conv:?}:"));
        }
        write_expr(frontend, &arg.value, out);
    }
}

fn write_block(frontend: &Frontend, block: &Block, out: &mut String) {
    out.push_str("(block");
    for stmt in &block.stmts {
        out.push(' ');
        write_stmt(frontend, stmt, out);
    }
    if let Some(tail) = &block.tail {
        out.push_str(" => ");
        write_expr(frontend, tail, out);
    }
    out.push(')');
}

fn write_stmt(frontend: &Frontend, stmt: &Stmt, out: &mut String) {
    match stmt {
        Stmt::Var { name, init, .. } => {
            out.push_str("(var ");
            out.push_str(frontend.text(*name));
            out.push(' ');
            write_expr(frontend, init, out);
            out.push(')');
        }
        Stmt::Set { place, value, .. } => {
            out.push_str("(set ");
            write_expr(frontend, place, out);
            out.push(' ');
            write_expr(frontend, value, out);
            out.push(')');
        }
        Stmt::Yield { value, .. } => {
            out.push_str("(yield ");
            write_expr(frontend, value, out);
            out.push(')');
        }
        Stmt::Bind { name, value, .. } => {
            out.push_str("(bind ");
            out.push_str(&pattern_text(frontend, name));
            out.push(' ');
            write_expr(frontend, value, out);
            out.push(')');
        }
        Stmt::TryBind { name, value, .. } => {
            out.push_str("(let? ");
            out.push_str(frontend.text(*name));
            out.push(' ');
            write_expr(frontend, value, out);
            out.push(')');
        }
        Stmt::When { cond, .. } => {
            out.push_str("(when ");
            write_cond(frontend, cond, out);
            out.push(')');
        }
        Stmt::For {
            pattern,
            iter,
            body,
            ..
        } => {
            out.push_str("(for ");
            out.push_str(&pattern_text(frontend, pattern));
            out.push(' ');
            write_expr(frontend, iter, out);
            out.push(' ');
            write_block(frontend, body, out);
            out.push(')');
        }
        Stmt::Let(decl) => {
            out.push_str("(let ");
            out.push_str(frontend.text(decl.name));
            out.push(')');
        }
        Stmt::Expr(e) => write_expr(frontend, e, out),
        Stmt::Error(_) => out.push_str("<error>"),
    }
}

fn write_cond(frontend: &Frontend, cond: &Cond, out: &mut String) {
    match cond {
        Cond::Expr(e) => write_expr(frontend, e, out),
        Cond::Bind { pattern, value, .. } => {
            out.push_str("(bind ");
            out.push_str(&pattern_text(frontend, pattern));
            out.push(' ');
            write_expr(frontend, value, out);
            out.push(')');
        }
    }
}

fn pattern_text(frontend: &Frontend, pattern: &rill_ast::Pattern) -> String {
    match pattern {
        rill_ast::Pattern::Wildcard(_) => "_".to_string(),
        rill_ast::Pattern::Binding { name, .. } => frontend.text(*name).to_string(),
        rill_ast::Pattern::Literal { text, .. } => text.clone(),
        rill_ast::Pattern::Constructor { path, args, .. } => {
            let head: Vec<&str> = path.iter().map(|s| frontend.text(*s)).collect();
            let args: Vec<String> = args.iter().map(|a| pattern_text(frontend, a)).collect();
            format!("({} {})", head.join("::"), args.join(" "))
        }
        rill_ast::Pattern::Tuple { elems, .. } => {
            let args: Vec<String> = elems.iter().map(|a| pattern_text(frontend, a)).collect();
            format!("({})", args.join(" "))
        }
        rill_ast::Pattern::Error(_) => "<pattern-error>".to_string(),
    }
}

fn write_ty(frontend: &Frontend, ty: &rill_ast::Type, out: &mut String) {
    match ty {
        rill_ast::Type::Path { segments, .. } => {
            let parts: Vec<&str> = segments.iter().map(|s| frontend.text(*s)).collect();
            out.push_str(&parts.join("::"));
        }
        rill_ast::Type::App { ctor, args, .. } => {
            out.push('(');
            write_ty(frontend, ctor, out);
            for a in args {
                out.push(' ');
                write_ty(frontend, a, out);
            }
            out.push(')');
        }
        rill_ast::Type::Tuple { elems, .. } => {
            out.push_str("(tuple");
            for e in elems {
                out.push(' ');
                write_ty(frontend, e, out);
            }
            out.push(')');
        }
        rill_ast::Type::Fn { .. } => out.push_str("(fn ...)"),
        rill_ast::Type::Ptr { mutable, .. } => {
            out.push_str(if *mutable { "&raw mut" } else { "&raw" })
        }
        rill_ast::Type::Dyn { .. } => out.push_str("dyn"),
        rill_ast::Type::Array { .. } => out.push_str("[_]"),
        rill_ast::Type::Error(_) => out.push_str("<type-error>"),
    }
}

// ---------------------------------------------------------------------------

#[test]
fn handler_forms_are_not_method_desugared() {
    let (f, _) = frontend(
        "let try_transfers () = {
            _ := option { transfer[inout alice, inout bob, 800] | transfer[inout alice, inout bob, 200] };
        };\n",
    );
    let body = dump(&f, &body_of(&f, "try_transfers"));
    assert!(body.contains("(handler option"), "{body}");
    assert!(
        body.contains("(index transfer Inout:alice Inout:bob 800)"),
        "{body}"
    );
    assert!(
        body.contains("(Choice "),
        "the `|` choice survives as an expression: {body}"
    );
}

#[test]
fn all_handler_keeps_guards() {
    let (f, _) =
        frontend("let pairs () = all { x := 1..5; y := 1..x; when (x + y = 5); (x, y) };\n");
    let body = dump(&f, &body_of(&f, "pairs"));
    assert!(body.starts_with("(handler all"), "{body}");
    assert!(
        body.contains("(when "),
        "guards stay for the handler: {body}"
    );
    assert!(body.contains("(RangeInclusive 1 5)"), "{body}");
    assert!(body.contains("(tuple x y)"), "{body}");
}

#[test]
fn concurrent_forms_collect_their_arms() {
    let (f, _) = frontend(
        "let combos () = {
            sync { fetch_a (); fetch_b () };
            race { fetch_a (); fetch_b () };
            rush { fetch_a (); fetch_b () };
            branch { fetch_a (); fetch_b () }
        };\n",
    );
    let body = dump(&f, &body_of(&f, "combos"));
    for kind in ["sync", "race", "rush", "branch"] {
        assert!(
            body.contains(&format!("(concurrent {kind}")),
            "{kind}: {body}"
        );
    }
    assert!(
        body.contains("(concurrent sync (fetch_a) (fetch_b))"),
        "{body}"
    );
}

#[test]
fn builder_protocol_matches_the_table() {
    let (f, _) = frontend(
        "builder seq = {
            let yield_val (x: a) -> Step a <computes> = Step::Yield x;
            let combine (s1: Step a, s2: () -> Step a) -> Step a <computes> = Step::Chain s1 s2;
            let ret (u: ()) -> Step a <computes> = Step::Done;
        };

        let evens (n: i32) = seq {
            for (i : 0..n) {
                if (i % 2 = 0) { yield i } else { () }
            }
        };\n",
    );
    let body = dump(&f, &body_of(&f, "evens"));
    // `stmt1; rest` -> `combine ([[stmt1]]) (delay { [[rest]] })`
    assert!(body.starts_with("(seq::combine"), "{body}");
    assert!(
        body.contains("(seq::delay (lambda () (block => (seq::ret unit))))"),
        "{body}"
    );
    // `for (i : 0..n) body` -> `for_each (0..n) { i -> [[body]] }`
    assert!(
        body.contains("(seq::for_each (RangeInclusive 0 n)"),
        "{body}"
    );
    // `yield i` -> `yield_val i`
    assert!(body.contains("(seq::yield_val i)"), "{body}");
    // No raw protocol statements survive desugaring.
    assert!(!body.contains("(yield "), "{body}");
    assert!(!body.contains("(for "), "{body}");
}

#[test]
fn bindings_and_try_bindings_desugar_to_closures() {
    let (f, _) = frontend(
        "builder seq = {
            let bind (x: a, f: (a) -> Step b) -> Step b <computes> = Step::Bind x f;
            let try_bind (x: Option a, f: (a) -> Step b) -> Step b <computes> = Step::Try x f;
            let ret (u: ()) -> Step a <computes> = Step::Done;
        };

        let reads () = seq {
            x := 1;
            let? y = maybe ();
            x + y
        };\n",
    );
    let body = dump(&f, &body_of(&f, "reads"));
    assert!(body.starts_with("(seq::bind 1 (lambda (x)"), "{body}");
    assert!(
        body.contains("(seq::try_bind (maybe) (lambda (y)"),
        "{body}"
    );
}

#[test]
fn standard_builders_desugar_without_declarations() {
    let (f, _) = frontend(
        "let saxpy (a: f32, x: Slice f32, y: inout Slice f32) <computes> = {
            simd {
                lane := lanes 8;
                store (inout y[0]) (fma a x lane)
            }
        };\n",
    );
    let body = dump(&f, &body_of(&f, "saxpy"));
    assert!(
        body.contains("(simd::bind (lanes 8) (lambda (lane)"),
        "{body}"
    );
    assert!(body.contains("(simd::ret "), "{body}");
}

#[test]
fn and_or_expand_to_if() {
    let (f, _) = frontend("let f (a: Bool, b: Bool, c: Bool) = a and b or c;\n");
    let body = dump(&f, &body_of(&f, "f"));
    // `or` is looser: `(a and b) or c` -> `if (if a { b } else { false }) { true } else { c }`
    assert!(
        body.starts_with("(if (if a (block => b) (block => false)) (block => true) (block => c))"),
        "{body}"
    );
}

#[test]
fn unsupported_statements_in_a_builder_block_are_reported() {
    let (f, sm) = parse_and_lower_text(
        "unsupported.rill",
        "builder seq = { let ret (u: ()) -> Step a <computes> = Step::Done; };
         let f () = seq { var x = 1; () };\n",
    );
    assert!(
        f.diagnostics.has_errors(),
        "must report the unsupported statement"
    );
    let rendered = rill_diagnostics::render_all(&sm, &f.diagnostics);
    assert!(rendered.contains("builder-protocol"), "{rendered}");
}

#[test]
fn plain_blocks_and_lambdas_are_left_alone() {
    let (f, _) = frontend("let fold_them (xs: Array i32) = xs . fold 0 { acc, x -> acc + x };\n");
    let body = dump(&f, &body_of(&f, "fold_them"));
    assert!(body.contains("(lambda (acc x)"), "{body}");
    assert!(
        !body.contains("::bind"),
        "ordinary closures are not builder calls: {body}"
    );
}

#[test]
fn match_arms_keep_their_patterns_and_bodies() {
    let (f, _) = frontend(
        "let classify (p: Pair) = match p {\n\
             | Pair (a, b) if a > b -> a,\n\
             | Pair (a, b) -> b,\n\
             _ -> 0\n\
         };\n",
    );
    let body = dump(&f, &body_of(&f, "classify"));
    assert!(body.contains("(match p"), "{body}");
    assert_eq!(
        body.matches("(arm ").count(),
        3,
        "three arms survive lowering: {body}"
    );
    assert!(
        body.contains("(arm (Pair (a b)) (guard (Gt a b)) a)"),
        "guard keeps its condition: {body}"
    );
    assert!(body.contains("(arm _ 0)"), "wildcard arm: {body}");
}
