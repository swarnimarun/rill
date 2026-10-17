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
        "let main () -> i32 <decides> = {\n    option { 1 };\n    0\n};\n",
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
