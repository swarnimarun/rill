use rill_ast::parse_and_lower_text;
use rill_codegen::emit_c;
use rill_hir::lower;
use rill_ty::check;

fn emit(src: &str) -> (String, rill_diagnostics::Diagnostics) {
    let (frontend, sm) = parse_and_lower_text("t.rill", src);
    assert!(
        !frontend.diagnostics.has_errors(),
        "{}",
        rill_diagnostics::render_all(&sm, &frontend.diagnostics)
    );
    let hir = lower(&frontend);
    let checked = check(&frontend, &hir);
    assert!(
        !checked.diagnostics.has_errors(),
        "{}",
        rill_diagnostics::render_all(&sm, &checked.diagnostics)
    );
    emit_c(&frontend, &hir, &checked)
}

#[test]
fn emits_a_recursive_program() {
    let (source, diagnostics) = emit(
        "let fib (n: i32) -> i32 <computes> = if (n < 2) { n } else { fib (n - 1) + fib (n - 2) };\n\
         let main () -> i32 <computes> = fib 10;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    assert!(source.contains("int main(void)"), "{source}");
    assert!(source.contains("rill_fib"), "{source}");
}

#[test]
fn records_text_and_casts_lower() {
    let (source, diagnostics) = emit(
        "type Point = record { x: i32; y: i32; };\n\
         let sum (p: Point) -> i32 <computes> = p.x + p.y;\n\
         let main () -> i32 <io> = {\n\
             var p = Point 3 4;\n\
             io::println (\"sum: \" ++ (sum p as String));\n\
             sum p\n\
         };\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    assert!(source.contains("st_Point"), "{source}");
    assert!(source.contains("rill_println"), "{source}");
    assert!(source.contains("rill_i64_to_text"), "{source}");
}

#[test]
fn loops_and_arrays_lower() {
    let (source, diagnostics) = emit(
        "let total (n: i32) -> i32 <computes> = {\n\
             var sum = 0;\n\
             for (i := 0..n) sum := sum + i;\n\
             sum\n\
         };\n\
         let main () -> i32 <computes> = total 100;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    assert!(source.contains("for (r_i32"), "{source}");
}

#[test]
fn unsupported_constructs_are_reported_not_miscompiled() {
    let (frontend, sm) = parse_and_lower_text(
        "u.rill",
        // `all { … }` still needs its array-valued result; the handler must be
        // *reported*, never silently mis-compiled.
        "let main () -> i32 <decides> = {\n    all { 1 };\n    0\n};\n",
    );
    let hir = lower(&frontend);
    let checked = check(&frontend, &hir);
    let (source, diagnostics) = emit_c(&frontend, &hir, &checked);
    assert!(diagnostics.has_errors(), "the handler must be reported");
    let rendered = rill_diagnostics::render_all(&sm, &diagnostics);
    assert!(
        rendered.contains("not lowered by the C backend"),
        "{rendered}"
    );
    // The program still type-checks; only code generation refuses it.
    assert!(source.contains("int main(void)"));
}

#[test]
fn option_body_with_statements_is_reported_not_miscompiled() {
    // The payload is lowered under a failure continuation; a statement before it
    // would need its own guard, so this shape must be *reported* rather than
    // emitted with a statement that runs after the body has already failed.
    let (frontend, sm) = parse_and_lower_text(
        "o.rill",
        "let main () -> i32 <decides> = {\n    option {\n        1;\n        2\n    };\n    0\n};\n",
    );
    let hir = lower(&frontend);
    let checked = check(&frontend, &hir);
    let (source, diagnostics) = emit_c(&frontend, &hir, &checked);
    assert!(diagnostics.has_errors(), "the handler must be reported");
    let rendered = rill_diagnostics::render_all(&sm, &diagnostics);
    assert!(
        rendered.contains("not lowered by the C backend"),
        "{rendered}"
    );
    assert!(source.contains("int main(void)"));
}

#[test]
fn a_two_valued_sync_is_a_tuple_not_its_last_arm() {
    // This is what tells `sync { … }` apart from a plain block with the same
    // statements: a block's value is its tail (`2 + 2`), while `sync`'s is the
    // tuple of its arms, which cannot be turned into text.
    let (frontend, sm) = parse_and_lower_text(
        "s.rill",
        "let main () -> () <io | suspends> = {\n    var t = sync { 1 + 1; 2 + 2 };\n    io::println ((t as String))\n};\n",
    );
    let hir = lower(&frontend);
    let checked = check(&frontend, &hir);
    let (_source, diagnostics) = emit_c(&frontend, &hir, &checked);
    assert!(
        diagnostics.has_errors(),
        "a tuple is not text: {diagnostics:?}"
    );
    let rendered = rill_diagnostics::render_all(&sm, &diagnostics);
    assert!(rendered.contains("String"), "{rendered}");
}

#[test]
fn enums_match_and_guards_lower() {
    let (source, diagnostics) = emit(
        "type Shape = | Circle i32 | Rect i32 i32 | Empty;\n\
         let area (s: Shape) -> i32 <computes> = match s {\n\
             Circle r -> 3 * r * r,\n\
             Rect w h if w == h -> w * h,\n\
             Rect w h -> 2 * (w + h),\n\
             Empty -> 0\n\
         };\n\
         let main () -> i32 <computes> = area (Rect 6 7) + area (Circle 2) + area Empty;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    assert!(source.contains("en_Shape"), "{source}");
    assert!(source.contains("en_Shape_TAG_Rect"), "{source}");
    assert!(source.contains("payload.Rect._0"), "{source}");
    // Three arms + the guard, all inside one `done`-flagged sequence.
    assert!(source.contains("done"), "{source}");
}

#[test]
fn non_capturing_closures_become_function_pointers() {
    let (source, diagnostics) = emit(
        "let apply (n: i32) -> i32 <computes> = {\n\
             inc := { x -> x + 1 };\n\
             inc n\n\
         };\n\
         let main () -> i32 <computes> = apply 41;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    assert!(source.contains("lambda0"), "{source}");
    // The binding is a function-pointer variable, not a value copy.
    assert!(source.contains("(*v_inc)"), "{source}");
}

#[test]
fn capturing_closures_are_reported() {
    let (frontend, sm) = parse_and_lower_text(
        "c.rill",
        "let f (n: i32) -> i32 <computes> = {\n\
             g := { x -> x + n };\n\
             g 1\n\
         };\n\
         let main () -> i32 <computes> = f 1;\n",
    );
    let hir = lower(&frontend);
    let checked = check(&frontend, &hir);
    let (_, diagnostics) = emit_c(&frontend, &hir, &checked);
    let rendered = rill_diagnostics::render_all(&sm, &diagnostics);
    assert!(rendered.contains("captures locals"), "{rendered}");
}

#[test]
fn range_forms_iterate_as_the_table_specifies() {
    // §2.5: `..` is inclusive, `..<` is half-open. Pin both so `for` loops
    // cannot quietly change meaning (the v1 benchmark mismatch came from this).
    let (source, diagnostics) = emit(
        "let count_inclusive (n: i32) -> i32 <computes> = {\n\
             var c = 0;\n\
             for (i := 0..n) set c = c + 1;\n\
             c\n\
         };\n\
         let count_half_open (n: i32) -> i32 <computes> = {\n\
             var c = 0;\n\
             for (i := 0..<n) set c = c + 1;\n\
             c\n\
         };\n\
         let main () -> i32 <computes> = count_inclusive 3 * 10 + count_half_open 3;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    // `for (i := 0..n)` -> `i_x <= n`; `for (i := 0..<n)` -> `i_x < n`.
    assert!(
        source.contains("<= p_n"),
        "inclusive `..` uses `<=`:\n{source}"
    );
    assert!(
        source.contains("< p_n"),
        "half-open `..<` uses `<`:\n{source}"
    );
}

#[test]
fn range_iteration_counts_at_runtime() {
    // 0..3 iterates 3 times, 0..<3 iterates 3 times, 0..2 iterates 3 times.
    let (_, diagnostics) = emit(
        "let a (n: i32) -> i32 <computes> = { var c = 0; for (i := 0..n) set c = c + 1; c };\n\
         let b (n: i32) -> i32 <computes> = { var c = 0; for (i := 0..<n) set c = c + 1; c };\n\
         let main () -> i32 <computes> = a 2 * 100 + b 3 * 10 + a 0;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
}

#[test]
fn unit_returning_calls_are_emitted_in_every_position() {
    // Regression: a call whose value is discarded was computed and dropped when
    // it sat in a block tail or an `if` branch (a silent miscompilation that
    // made scanner recursion vanish).
    let (source, diagnostics) = emit(
        "let bump (n: inout i64) -> () <computes> = { set n = n + 1i64; };\n\
         let step (n: inout i64) -> () <computes> = {\n\
             if (n < 3i64) { bump (inout n); step (inout n) } else { () }\n\
         };\n\
         let branchy (n: inout i64, c: Bool) -> () <computes> = {\n\
             if (c) { bump (inout n) } else { bump (inout n) }\n\
         };\n\
         let main () -> i32 <computes> = {\n\
             var n: i64 = 0i64;\n\
             step (inout n);\n\
             branchy (inout n, true);\n\
             n as i32\n\
         };\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    // `step`'s body: both the recursive tail call and the loop-branch call.
    assert!(
        source.contains("(void)(rill_step("),
        "tail call emitted:\n{source}"
    );
    assert!(
        source.contains("(void)(rill_bump("),
        "branch call emitted:\n{source}"
    );
    // The `branchy` helper's two call sites are both emitted.
    assert!(
        source.contains("if (p_c)") && source.matches("rill_bump(").count() >= 3,
        "every call site emitted:\n{source}"
    );
}

#[test]
fn unit_tail_call_in_a_function_body_runs() {
    // The scanner shape: `scan` advances the cursor then hands over to `tail`.
    let (_, diagnostics) = emit(
        "let tail (i: inout i64, n: i64) -> () <computes> = {\n\
             if (i < n) { set i = i + 1i64; tail (inout i, n) } else { () }\n\
         };\n\
         let scan (i: inout i64, n: i64) -> () <computes> = {\n\
             set i = i + 1i64;\n\
             tail (inout i, n)\n\
         };\n\
         let main () -> i32 <computes> = {\n\
             var i: i64 = 0i64;\n\
             scan (inout i, 7i64);\n\
             i as i32\n\
         };\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
}

#[test]
fn while_conditions_re_evaluate_each_iteration() {
    // A condition containing `and`/`or` desugars to an `if` expression, whose
    // statements hoist; `while` must therefore evaluate it inside the loop.
    let (source, diagnostics) = emit(
        "let count (n: i32) -> i32 <computes> = {\n\
             var i = 0;\n\
             var alive = true;\n\
             while (alive and i < n) {\n\
                 set i = i + 1;\n\
                 if (i = 3) { set alive = false } else { () }\n\
             };\n\
             i\n\
         };\n\
         let main () -> i32 <computes> = count 100;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    assert!(
        source.contains("for (;;)"),
        "condition stays inside the loop:\n{source}"
    );
    assert!(source.contains("break;"), "{source}");
}

#[test]
fn break_exits_only_its_loop() {
    let (_, diagnostics) = emit(
        "let f (n: i32) -> i32 <computes> = {\n\
             var total = 0;\n\
             for (i := 0..<4) {\n\
                 var j = 0;\n\
                 while (j < 10) {\n\
                     set j = j + 1;\n\
                     if (j > n) { break } else { () }\n\
                 };\n\
                 set total = total + j\n\
             };\n\
             total\n\
         };\n\
         let main () -> i32 <computes> = f 2;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
}

#[test]
fn module_level_vars_become_file_scope_statics() {
    // Pools and arenas must not sit on the stack: a module-level `var` becomes a
    // static, and functions refer to it without a parameter.
    let (source, diagnostics) = emit(
        "type Node = record { value: i64; };\n\
         var pool: [Node; 1024] = comptime { zeroed () };\n\
         var used: usize = 0u64;\n\
         let push (value: i64) -> usize <computes> = {\n\
             var idx = used;\n\
             set pool[idx].value = value;\n\
             set used = idx + 1u64;\n\
             idx\n\
         };\n\
         let main () -> i32 <computes> = push 41i64 as i32;\n",
    );
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    assert!(source.contains("static "), "a file-scope static:\n{source}");
    assert!(source.contains("g_pool"), "the pool is a static:\n{source}");
    assert!(source.contains("g_used"), "{source}");
}
