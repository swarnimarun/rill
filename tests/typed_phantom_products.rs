//! K2 acceptance assertions for finite Copy products with opaque logical arguments.
//! Prepared source-only: this file does not itself record an executed result.
use rill::{
    check, interpreter,
    modules::compile::load_program_with_sources,
    parser, typed,
    typed::provenance::{
        lower_main_with_function_origins, nodes::lower_main_with_copy_node_origins,
    },
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const MARK: &str = "type Mark<a> = Mark(i32)\n";
const STAMP: &str = "type Stamp<a> = { n: i32 }\n";
const FLAG: &str = "type Flag<a> = Off | On\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-phantom-products-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }

    fn build(&self, c: &str, compiler: &str, optimization: &str) -> Output {
        let path = self.write("main.c", c);
        Command::new(compiler)
            .args(["-std=c11", "-pedantic-errors", optimization, "-I"])
            .arg(&self.0)
            .arg(path)
            .arg("-o")
            .arg(self.0.join("main"))
            .output()
            .unwrap_or_else(|e| panic!("{compiler} {optimization}: {e}"))
    }

    fn run_c(&self, c: &str, expected: Option<&str>, trace: &str, context: &str) {
        for compiler in compilers() {
            for optimization in ["-O0", "-O2"] {
                let build = self.build(c, compiler, optimization);
                assert!(
                    build.status.success(),
                    "{context} ({compiler} {optimization}): {}",
                    String::from_utf8_lossy(&build.stderr)
                );
                let output = Command::new(self.0.join("main")).output().unwrap();
                match expected {
                    Some(expected) => {
                        assert!(
                            output.status.success(),
                            "{context} ({compiler} {optimization})"
                        );
                        assert_eq!(
                            output.stdout,
                            format!("{expected}\n").as_bytes(),
                            "{context}"
                        );
                        assert_eq!(output.stderr, trace.as_bytes(), "{context}");
                    }
                    None => {
                        assert!(
                            !output.status.success(),
                            "{context} ({compiler} {optimization})"
                        );
                        assert!(output.stdout.is_empty(), "{context}");
                        assert_eq!(
                            output.stderr,
                            format!("{trace}rill: runtime trap\n").as_bytes(),
                            "{context}"
                        );
                    }
                }
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Remove only this test's own temporary files after later authorized execution.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compilers() -> &'static [String] {
    static COMPILERS: OnceLock<Vec<String>> = OnceLock::new();
    COMPILERS.get_or_init(|| {
        let mut available = Vec::new();
        for compiler in ["gcc", "clang"] {
            match Command::new(compiler).arg("--version").output() {
                Ok(output) if output.status.success() => {
                    eprintln!("K2 C compiler: {}", String::from_utf8_lossy(&output.stdout));
                    available.push(compiler.to_string());
                }
                _ => eprintln!("SKIP K2 cross-compiler evidence: {compiler} unavailable"),
            }
        }
        // Missing configurations remain explicit skips, never cc substitutes
        // for evidence that specifically requires both GCC and Clang.
        available
    })
}

fn lower(program: &rill::ast::Program, source: &str) -> (typed::TypedMain, String) {
    let snapshot = program.clone();
    let checked = check::analyze(program).unwrap_or_else(|e| panic!("{source}: {e}"));
    let lowered = typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
    typed::verify(&lowered).unwrap_or_else(|e| panic!("{source}: {e}"));
    let c = typed::emit_c(&lowered).unwrap();
    assert_eq!(rill::backend::emit(program).unwrap(), c, "{source}");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    assert!(
        !c.contains("restrict"),
        "Copy slots confer no pointee noalias proof"
    );
    assert_eq!(
        typed::emit_c(&typed::lower_main(&checked).unwrap()).unwrap(),
        c,
        "repeated lowering: {source}"
    );
    assert_eq!(*program, snapshot, "lowering changed the checked source");
    (lowered, c)
}

fn compare_program(program: &rill::ast::Program, source: &str, expected: Option<&str>) {
    let (lowered, c) = lower(program, source);
    let actual = typed::evaluate(&lowered);
    match expected {
        Some(expected) => assert_eq!(actual.as_ref().unwrap().to_string(), expected, "{source}"),
        None => assert_eq!(actual.as_ref().unwrap_err(), "runtime trap", "{source}"),
    }
    assert_eq!(interpreter::run(program), actual, "{source}");
    assert_eq!(interpreter::run_reference(program), actual, "{source}");
    // The independent uniform C oracle has a narrower open-product layout gate.
    // Its rejection is not evidence that ordinary typed lowering may fall back.
    Fixture::new().run_c(&c, expected, "", source);
}

fn compare(source: &str, expected: Option<&str>) {
    let program = parser::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    compare_program(&program, source, expected);
}

fn metamorphic(source: &str, expected: &str) {
    compare(source, Some(expected));
    compare(
        &format!("type Unused = Extra(i32)\n{source}"),
        Some(expected),
    );
}

fn rejects_source(source: &str, prefix: &str) {
    let program = parser::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let error = check::analyze(&program).err().unwrap();
    assert!(error.starts_with(prefix), "{source}: {error}");
    assert_eq!(interpreter::run(&program).unwrap_err(), error, "{source}");
    assert_eq!(
        rill::backend::emit(&program).unwrap_err(),
        error,
        "{source}"
    );
}

#[test]
fn fixed_payload_enum_and_record_roots_keep_real_returned_values() {
    for (source, expected) in [
        (format!("{MARK}fn main() {{ match Mark(42) {{ Mark(n) => n }} }}"), "42"),
        (format!("{MARK}fn make() {{ Mark(42) }}\nfn forward() {{ make() }}\nfn main() {{ forward() }}"), "Mark(42)"),
        (format!("{STAMP}fn make() {{ record Stamp {{ n = 42 }} }}\nfn main() {{ let weak = make(); weak.n }}"), "42"),
        (format!("{STAMP}fn main() {{ record Stamp {{ n = 42 }} }}"), "Stamp { n = 42 }"),
        ("type Empty<a,b> = {}\nfn make() { record Empty {} }\nfn main() { make() }".into(), "Empty {}"),
        ("type Mixed<a> = Quiet | Loud(i32)\nfn main() { let m = if false { Quiet } else { Loud(42) }; match m { Quiet => 0 | Loud(n) => n } }".into(), "42"),
        ("type Keep<a,b> = Keep(a)\nfn wrap(x) { Keep(x) }\nfn main() { match wrap(42) { Keep(n) => n } }".into(), "42"),
    ] {
        metamorphic(&source, expected);
    }
}

#[test]
fn weak_calls_conditionals_matches_and_unused_results_remain_evaluated() {
    for initializer in [
        "make(inout count)",
        "if true { make(inout count) } else { record Stamp { n = 0 } }",
        "match true { true => make(inout count) | false => record Stamp { n = 0 } }",
    ] {
        compare(
            &format!("{STAMP}fn make(inout count) {{ count <- count + 1; record Stamp {{ n = 40 }} }}\nfn main() {{ let mut count = 0; let weak = {initializer}; let unused = make(inout count); weak.n + count }}"),
            Some("42"),
        );
    }
    // This is the exact K1 fixed-record negative, promoted without closing a.
    metamorphic("type Stamp<a> = { number: i32 }\nfn main() { let weak = if true { record Stamp { number = 42 } } else { record Stamp { number = 0 } }; weak.number }", "42");
}

#[test]
fn finite_copy_fields_include_strings_buffers_vectors_and_closed_descendants() {
    for (source, expected) in [
        ("type Text<a> = Text(str)\nfn main() { match Text(\"é\") { Text(s) => byte_len(s) + 40 } }", "42"),
        ("type Buffer<a,b> = Buffer(a)\nfn make() { Buffer(bytes_set(bytes_new(2), 1, 42)) }\nfn main() { match make() { Buffer(b) => bytes_get(b,1) } }", "42"),
        ("type Vector<a> = Vector(u8x16)\nfn main() { match Vector(u8x16_load(bytes_set(bytes_new(16), 5, 42),0)) { Vector(v) => u8x16_lane(v,5) } }", "42"),
        ("type Fixed = Fixed(i32)\ntype Inner = { value: Fixed }\ntype Outer<a> = Outer(Inner)\nfn main() { match Outer(record Inner { value = Fixed(42) }) { Outer(r) => match r.value { Fixed(n) => n } } }", "42"),
    ] {
        metamorphic(source, expected);
    }
}

#[test]
fn nested_symbolic_fields_reconstruct_complete_substituted_identities() {
    // Link stores Flag<opaque> and Flag<Flag<opaque>>, not two identical tags.
    metamorphic("type Flag<a> = Off | On\ntype Link<a> = Link(a,Flag<a>)\nfn tag(x) { match Link(x,Off) { Link(_,f) => f } }\nfn main() { tag(Off) }", "Off");
    for declarations in [
        "type Flag<a> = Off | On\ntype Option<a> = None | Some(a)\ntype Boxed<a> = Boxed(Flag<Option<a>>)\ntype Nest<a> = { item: Boxed<a> }",
        "type Nest<a> = { item: Boxed<a> }\ntype Boxed<a> = Boxed(Flag<Option<a>>)\ntype Option<a> = None | Some(a)\ntype Flag<a> = Off | On",
    ] {
        metamorphic(&format!("{declarations}\nfn make() {{ record Nest {{ item = Boxed(On) }} }}\nfn main() {{ let weak = make(); match weak.item {{ Boxed(On) => 42 | Boxed(Off) => 0 }} }}"), "42");
    }
    metamorphic("type Flag<a> = Off | On\ntype Pair<a,b> = Pair(Flag<b>,Flag<a>,Flag<b>)\ntype Outer<a,b> = Outer(Pair<b,a>)\nfn main() { match Outer(Pair(On,Off,On)) { Outer(Pair(On,Off,On)) => 42 | _ => 0 } }", "42");
}

#[test]
fn option_of_symbolic_flag_checks_both_variants_and_accepts_each_path() {
    for choose_none in ["true", "false"] {
        metamorphic(&format!("{FLAG}type Option<a> = None | Some(a)\nfn choose(yes) {{ if yes {{ None }} else {{ Some(On) }} }}\nfn main() {{ match choose({choose_none}) {{ None => 42 | Some(On) => 42 | Some(Off) => 0 }} }}"), "42");
    }
    metamorphic(&format!("{FLAG}type Option<a> = None | Some(a)\nfn make() {{ Some(Some(Off)) }}\nfn main() {{ match make() {{ Some(Some(Off)) => 42 | Some(Some(On)) => 0 | Some(None) => 0 | None => 0 }} }}"), "42");
}

#[test]
fn source_order_once_only_arguments_and_scrutinees_survive_product_layouts() {
    compare("type Stamp<a> = { first: i32; second: i32 }\nfn tick(inout count) { count <- count + 1; count }\nfn main() { let mut count = 0; let weak = if true { record Stamp { second = tick(inout count); first = tick(inout count) } } else { record Stamp { first = 0; second = 0 } }; weak.first * 10 + weak.second + count * 10 + 1 }", Some("42"));
    compare("type Pair<a> = Pair(i32,i32)\nfn tick(inout count) { count <- count + 1; count }\nfn main() { let mut count = 0; let n = match Pair(tick(inout count),tick(inout count)) { Pair(first,second) => first * 10 + second }; n + count * 15 }", Some("42"));
    compare(&format!("{MARK}fn make(inout count) {{ count <- count + 1; Mark(41) }}\nfn main() {{ let mut count = 0; let n = match make(inout count) {{ Mark(n) => n }}; n + count }}"), Some("42"));
    compare(&format!("{MARK}fn ints(x: Mark<i32>) {{ match x {{ Mark(n) => n }} }}\nfn bools(x: Mark<bool>) {{ match x {{ Mark(n) => n }} }}\nfn main() {{ let mut count = 0; let snapshot = {{ count <- count + 1; count }}; let shape = Mark(snapshot); let alias = shape; count <- 20; ints(alias) + bools(shape) + count * 2 }}"), Some("42"));
}

#[test]
fn product_root_and_field_loans_preserve_slots_siblings_and_snapshots() {
    for source in [
        "type Stamp<a> = { n: i32 }\nfn get(read p) { p }\nfn forward(read p) { get(p) }\nfn replace(inout p,next) { p <- next }\nfn write_forward(inout p,next) { replace(inout p,next) }\nfn main() { let mut p = record Stamp { n = 20 }; let old = forward(p); write_forward(inout p,record Stamp { n = 22 }); old.n + p.n }",
        "type Stamp<a> = { n: i32 }\nfn bump(inout n) { n <- n + 1 }\nfn forward(inout n) { bump(inout n) }\nfn get(read n) { n }\nfn main() { let mut p = record Stamp { n = 41 }; forward(inout p.n); get(p.n) }",
        "type Pair<a> = { x: i32; y: i32 }\nfn swap(inout a,inout b) { let old = a; a <- b; b <- old }\nfn main() { let mut p = record Pair { x = 20; y = 22 }; swap(inout p.x,inout p.y); p.x + p.y }",
        "type Pair<a> = { x: i32; y: i32 }\nfn main() { let mut p = record Pair { x = 1; y = 2 }; p.x <- { p <- record Pair { x = 7; y = 8 }; 34 }; p.x + p.y }",
        "type Inner<a> = { x: i32; y: i32 }\ntype Outer<a> = { inner: Inner<a>; tail: i32 }\nfn main() { let mut p = record Outer { inner = record Inner { x = 1; y = 2 }; tail = 3 }; p.inner.x <- { p.inner <- record Inner { x = 30; y = 8 }; 31 }; p.inner.x + p.inner.y + p.tail }",
        "type Stamp<a> = { n: i32 }\nfn change(read old,inout n) { n <- n + 1; old + n }\nfn main() { let mut p = record Stamp { n = 20 }; change({ p.n + 1 },inout p.n) }",
    ] {
        compare(source, Some("42"));
    }
}

#[test]
fn recursive_forwarding_deduplicates_full_signatures_and_keeps_call_guard() {
    compare(&format!("{MARK}fn first(n,x) {{ if n == 0 {{ x }} else {{ second(n - 1,x) }} }}\nfn second(n,x) {{ if n == 0 {{ x }} else {{ first(n - 1,x) }} }}\nfn main() {{ match first(7,Mark(42)) {{ Mark(n) => n }} }}"), Some("42"));
    for (depth, expected) in [(30, Some("Stamp { n = 42 }")), (31, None)] {
        compare(&format!("{STAMP}fn recur(n,read p) {{ if n == 0 {{ p }} else {{ recur(n - 1,p) }} }}\nfn main() {{ recur({depth},record Stamp {{ n = 42 }}) }}"), expected);
    }
    let source = format!("{MARK}fn again(x) {{ if false {{ x }} else {{ again(x) }} }}\nfn main() {{ again(Mark(42)) }}");
    let program = parser::parse(&source).unwrap();
    let _ = lower(&program, &source);
    // Do not evaluate pure divergence to test discovery or no-return rejection.
}

#[test]
fn distinct_weak_scopes_closed_arguments_and_body_only_binders_are_not_merged() {
    compare(&format!("{MARK}fn make(n) {{ Mark(n) }}\nfn id(x) {{ x }}\nfn score(x) {{ match x {{ Mark(n) => n }} }}\nfn main() {{ let first = make(20); let second = make(2); score(id(first)) + score(id(first)) + score(id(second)) }}"), Some("42"));
    compare(&format!("{STAMP}fn independent(x) {{ let local = if true {{ record Stamp {{ n = 1 }} }} else {{ record Stamp {{ n = 2 }} }}; if local.n == 1 {{ x }} else {{ x }} }}\nfn main() {{ if independent(true) {{ independent(40) + independent(2) }} else {{ 0 }} }}"), Some("42"));
    compare(&format!("{MARK}fn ints(x: Mark<i32>) {{ match x {{ Mark(n) => n }} }}\nfn bools(x: Mark<bool>) {{ match x {{ Mark(n) => n }} }}\nfn main() {{ let value = Mark(21); ints(value) + bools(value) }}"), Some("42"));
}

#[test]
fn unused_products_and_selected_branches_preserve_runtime_traps() {
    for source in [
        format!("{MARK}fn make() {{ Mark(1 / 0) }}\nfn main() {{ let unused = make(); 42 }}"),
        format!("{STAMP}fn make() {{ record Stamp {{ n = 2147483647 + 1 }} }}\nfn main() {{ let unused = make(); 42 }}"),
        format!("{MARK}fn first(x,y) {{ x }}\nfn main() {{ first(Mark(42),1 / 0) }}"),
        format!("{MARK}fn main() {{ match Mark(42) {{ Mark(_) => 1 / 0 }} }}"),
        format!("{STAMP}fn main() {{ let mut p = record Stamp {{ n = 1 }}; p.n <- {{ p <- record Stamp {{ n = 42 }}; 1 / 0 }}; p.n }}"),
    ] {
        compare(&source, None);
    }
    compare(&format!("{MARK}fn main() {{ let value = if true {{ Mark(42) }} else {{ Mark(1 / 0) }}; match value {{ Mark(n) => n }} }}"), Some("42"));
}

#[test]
fn genuinely_unknown_physical_fields_stay_unsupported_after_no_normal_admission() {
    for source in [
        "type Option<a> = None | Some(a)\nfn main() { None }",
        "type Option<a> = None | Some(a)\nfn make() { None }\nfn main() { let unused = make(); 42 }",
        "type Option<a> = None | Some(a)\ntype Wrap<a> = Wrap(Option<a>)\nfn main() { match Wrap(None) { Wrap(None) => 42 | Wrap(Some(_)) => 0 } }",
        "type Option<a> = None | Some(a)\ntype Store<a> = { value: Option<a> }\nfn make() { record Store { value = None } }\nfn main() { let unused = make(); 42 }",
        "type Store<a> = { value: a }\nfn forever() { forever() }\nfn main() { let unused = record Store { value = forever() }; 42 }",
        "type Option<a> = None | Some(a)\nfn main() { if false { let weak = if true { None } else { None }; 0 } else { 42 } }",
    ] {
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap_or_else(|e| panic!("{source}: {e}"));
        let error = typed::lower_main(&checked).unwrap_err();
        assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{source}: {error}");
        // Source accepted does not mean representable, and these controls can diverge.
    }
}

#[test]
fn bare_nonreturning_results_preserve_finite_product_prefixes_and_branches() {
    for (source, expected) in [
        ("fn forever() { forever() }\nfn main() { let unused = forever(); 42 }".to_string(), None),
        ("fn forever() { forever() }\nfn main() { if false { let unused = forever(); 0 } else { 42 } }".to_string(), Some("42")),
        (format!("{MARK}fn forever() {{ forever() }}\nfn main() {{ let p = if true {{ Mark(42) }} else {{ Mark(0) }}; let unused = forever(); match p {{ Mark(n) => n }} }}"), None),
    ] {
        compare(&source, expected);
    }
}

#[test]
fn products_do_not_relax_value_restriction_nominality_or_exhaustiveness() {
    for binding in [
        "let p = make();",
        "let p = if true { Mark(1) } else { Mark(2) };",
        "let p = match true { _ => Mark(1) };",
        "let mut p = Mark(1);",
        "let first = make(); let second = first; let p = second;",
    ] {
        rejects_source(&format!("{MARK}fn make() {{ Mark(1) }}\nfn ints(x: Mark<i32>) {{ 40 }}\nfn bools(x: Mark<bool>) {{ 2 }}\nfn main() {{ {binding} ints(p) + bools(p) }}"), "E-TYPE:");
    }
    for (source, prefix) in [
        ("type First<a> = A(i32)\ntype Second<a> = B(i32)\nfn main() { if true { A(1) } else { B(1) } }", "E-TYPE:"),
        ("type Mixed<a> = Quiet | Loud(i32)\nfn main() { match Loud(42) { Loud(n) => n } }", "E-MATCH:"),
        ("type Mark<a> = Mark(i32)\nfn main() { Mark(1) == Mark(1) }", "E-TYPE:"),
        ("type Mark<a> = Mark(i32)\nfn ints(x: Mark<i32>) { 40 }\nfn bools(x: Mark<bool>) { 2 }\nfn keep(read x: Mark<i32>, y: i32) { y }\nfn main() { let x = Mark(1); keep(x,{ let alias = x; ints(alias) + bools(alias) }) }", "E-TYPE:"),
        ("type Flag<a> = Off | On\ntype Loop<a> = Loop(Flag<Loop<a>>)\nfn main() { 42 }", "E-TYPE:"),
    ] {
        rejects_source(source, prefix);
    }
    // The schema dependency traversal may name either member of this cycle.
    // Each entry point must still reject with the specific recursion diagnostic;
    // the other source cases above retain exact diagnostic-precedence equality.
    let source = "type A<a> = A(B<a>)\ntype B<a> = B(A<a>)\nfn main() { 42 }";
    let program = parser::parse(source).unwrap();
    for error in [
        check::analyze(&program).err().unwrap(),
        interpreter::run(&program).unwrap_err(),
        rill::backend::emit(&program).unwrap_err(),
    ] {
        assert!(
            matches!(
                error.as_str(),
                "E-TYPE: recursive type 'A' requires indirection"
                    | "E-TYPE: recursive type 'B' requires indirection"
            ),
            "{source}: {error}"
        );
    }
    for (first, second) in [
        ("ptr<void>", "const_ptr<void>"),
        ("ptr<First>", "ptr<Second>"),
        ("Wrap<i32>", "Wrap<bool>"),
    ] {
        rejects_source(&format!("{MARK}type Wrap<a> = Wrap(a)\nfn make() {{ Mark(1) }}\nfn first(x: Mark<{first}>) {{ 40 }}\nfn second(x: Mark<{second}>) {{ 2 }}\nfn main() {{ let weak = make(); first(weak) + second(weak) }}"), "E-TYPE:");
    }
}

#[test]
fn open_products_do_not_bypass_existing_owner_rejections() {
    // These are rejection controls, not tests of any unaccepted owned-record
    // extension. Closed accepted owner allocation/cleanup controls stay in
    // their existing suites and require their own authorized execution.
    for source in [
        "type Mark<a> = Mark(SystemBoxI32)\nfn main() { let owner = system_box_i32_abort(1); let p = Mark(move owner); let copied = p; 42 }",
        "type Stamp<a> = { owner: SystemBoxI32 }\nfn main() { let owner = system_box_i32_abort(42); let p = record Stamp { owner = move owner }; 42 }",
        "type Mark<a> = Mark(i32)\nfn get(x: Mark<SystemBoxI32>) { 42 }\nfn main() { let p = Mark(1); get(p) }",
        "extern fn foreign() -> i32;\ntype Mark<a> = Mark(i32)\nfn get(x: Mark<i32>) { match x { Mark(n) => n } }\nfn main() { let owner = system_box_i32_abort(1); get(Mark(41)) + box_read(owner) }",
    ] {
        let program = parser::parse(source).unwrap();
        // Preserve the existing applicable boundary rather than inventing a
        // source-vs-lowering classification for unsupported owner programs.
        assert!(interpreter::run(&program).is_err(), "{source}");
        assert!(rill::backend::emit(&program).is_err(), "{source}");
    }
}

#[test]
fn product_field_loans_reject_overlap_prefix_conflicts_and_read_to_write() {
    for body in [
        "both(inout p.x,inout p.x)",
        "observe(p.x,inout p.x)",
        "set(inout p.x,p.x)",
        "set(inout p.x,{ p <- record Pair { x = 1; y = 2 }; 42 })",
        "whole(p,inout p.x)",
    ] {
        rejects_source(&format!("type Pair<a> = {{ x: i32; y: i32 }}\nfn both(inout a,inout b) {{ a <- b }}\nfn observe(read a,inout b) {{ b <- a }}\nfn set(inout a,b) {{ a <- b }}\nfn whole(read a,inout b) {{ b <- 42 }}\nfn main() {{ let mut p = record Pair {{ x = 20; y = 22 }}; {body} }}"), "E-LOAN:");
    }
    rejects_source(&format!("{STAMP}fn replace(inout p,next) {{ p <- next }}\nfn forward(read p,next) {{ replace(inout p,next) }}\nfn main() {{ forward(record Stamp {{ n = 0 }},record Stamp {{ n = 42 }}) }}"), "E-LOAN:");
}

#[test]
fn imported_equal_layout_products_keep_distinct_resolved_names() {
    let fixture = Fixture::new();
    for name in ["left", "right"] {
        fixture.write(
            &format!("{name}.rill"),
            "export type Stamp<a> = { n: i32 }\nexport fn make(n) { record Stamp { n = n } }",
        );
    }
    let source = "import left; import right;\nfn main() { let a = left.make(20); let b = right.make(22); a.n + b.n }";
    let path = fixture.write("main.rill", source);
    let program = rill::modules::compile::load_program(&path).unwrap();
    compare_program(&program, source, Some("42"));
    let path = fixture.write("bad.rill", "import left; import right;\nfn main() { if true { left.make(42) } else { right.make(42) } }");
    let error = rill::modules::compile::load_program(&path).unwrap_err();
    assert_eq!(error.code, "E-MODULE-CHECK");
    assert!(error.message.starts_with("E-TYPE:"));
}

#[test]
fn optional_origins_explicitly_reject_opaque_products_after_ordinary_success() {
    for source in [
        format!("{MARK}fn main() {{ Mark(42) }}"),
        format!("{STAMP}fn make() {{ record Stamp {{ n = 42 }} }}\nfn main() {{ let p = make(); p.n }}"),
        format!("{FLAG}type Option<a> = None | Some(a)\nfn main() {{ let p = if true {{ None }} else {{ Some(On) }}; match p {{ None => 42 | Some(_) => 0 }} }}"),
    ] {
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", &source);
        let sources = load_program_with_sources(&path).unwrap();
        let (_, before) = lower(sources.program(), &source);
        for (error, index) in [
            (lower_main_with_function_origins(sources.clone()).unwrap_err(), "function"),
            (lower_main_with_copy_node_origins(sources.clone()).unwrap_err(), "Copy node"),
        ] {
            assert_eq!(error, format!("E-TYPED-UNSUPPORTED: {index} origin index does not map opaque logical arguments"), "{source}");
        }
        assert_eq!(rill::backend::emit(sources.program()).unwrap(), before);
    }
    let fixture = Fixture::new();
    let path = fixture.write("closed.rill", &format!("{MARK}fn get(x: Mark<i32>) {{ match x {{ Mark(n) => n }} }}\nfn main() {{ get(Mark(42)) }}"));
    let sources = load_program_with_sources(&path).unwrap();
    let expected = rill::backend::emit(sources.program()).unwrap();
    assert_eq!(
        typed::emit_c(
            lower_main_with_function_origins(sources.clone())
                .unwrap()
                .typed()
        )
        .unwrap(),
        expected
    );
    assert_eq!(
        typed::emit_c(lower_main_with_copy_node_origins(sources).unwrap().typed()).unwrap(),
        expected
    );
}

#[test]
fn optional_origin_errors_keep_ordinary_semantic_precedence() {
    for (tail, expected) in [
        ("let unused = forever(); match p { Mark(n) => n }", None),
        (
            "if false { let unused = forever(); match p { Mark(n) => n } } else { 42 }",
            Some("42"),
        ),
    ] {
        let source = format!("{MARK}fn forever() {{ forever() }}\nfn main() {{ let p = if true {{ Mark(42) }} else {{ Mark(0) }}; {tail} }}");
        let fixture = Fixture::new();
        let path = fixture.write("combined.rill", &source);
        let sources = load_program_with_sources(&path).unwrap();
        let (ordinary, emitted) = lower(sources.program(), &source);
        assert!(emitted.contains("static void rl_fn_"));
        match expected {
            Some(value) => assert_eq!(typed::evaluate(&ordinary).unwrap().to_string(), value),
            None => assert_eq!(typed::evaluate(&ordinary).unwrap_err(), "runtime trap"),
        }
        for error in [
            lower_main_with_function_origins(sources.clone()).unwrap_err(),
            lower_main_with_copy_node_origins(sources).unwrap_err(),
        ] {
            assert!(error.starts_with("E-TYPED-UNSUPPORTED:"), "{error}");
            assert!(error.contains("no-normal"), "{error}");
        }
    }
    for source in [
        "type Option<a> = None | Some(a)\nfn main() { None }",
        "type Option<a> = None | Some(a)\nfn make() { None }\nfn main() { let unused = make(); 42 }",
    ] {
        let fixture = Fixture::new();
        let path = fixture.write("main.rill", source);
        let sources = load_program_with_sources(&path).unwrap();
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = typed::lower_main(&checked).unwrap_err();
        assert_eq!(lower_main_with_function_origins(sources.clone()).unwrap_err(), ordinary);
        assert_eq!(lower_main_with_copy_node_origins(sources).unwrap_err(), ordinary);
    }
}

const NATIVE_HEADER: &str = r#"
#ifndef K2_NATIVE_H
#define K2_NATIVE_H
#include <stdint.h>
#include <stdio.h>
typedef struct KPHandle { int32_t n; } KPHandle;
static KPHandle kp_handle = { 42 };
static uint32_t kp_word(void) { fputs("word;", stderr); return UINT32_C(40); }
static KPHandle *kp_get(void) { fputs("get;", stderr); return &kp_handle; }
static const KPHandle *kp_const(void) { fputs("const;", stderr); return &kp_handle; }
static int32_t kp_read(const KPHandle *p) { fputs("read;", stderr); return p->n; }
static uint32_t kp_unused(uint32_t n) { return n; }
#endif
"#;
const NATIVE: &str = "@include(\"k2_native.h\");\nextern fn kp_word() -> u32;\nextern fn kp_get() -> ptr<KPHandle>;\nextern fn kp_const() -> const_ptr<KPHandle>;\nextern fn kp_read(p: const_ptr<KPHandle>) -> i32;\nextern fn kp_unused(n: u32) -> u32;\n";

#[test]
fn native_fixed_fields_keep_full_manifests_pointer_identity_and_const_slots() {
    let fixture = Fixture::new();
    fixture.write("k2_native.h", NATIVE_HEADER);
    let source = format!("{NATIVE}type Packet<a> = {{ word: u32; pointer: ptr<KPHandle>; shared: const_ptr<KPHandle> }}\nfn make() {{ record Packet {{ shared = kp_const(); word = kp_word(); pointer = kp_get() }} }}\nfn snapshot(read p) {{ p }}\nfn replace(inout p,next) {{ p <- next }}\nfn main() {{ let mut p = make(); let old = snapshot(p.shared); replace(inout p.pointer,c_null()); if is_null(p.pointer) && p.word == u32(40) {{ kp_read(old) }} else {{ 0 }} }}");
    let program = parser::parse(&source).unwrap();
    let (lowered, c) = lower(&program, &source);
    let error = typed::evaluate(&lowered).unwrap_err();
    assert_eq!(
        error,
        "E-INTERP-FFI: cannot call C function kp_const in the interpreter"
    );
    assert_eq!(interpreter::run(&program).unwrap_err(), error);
    assert_eq!(interpreter::run_reference(&program).unwrap_err(), error);
    assert!(c.contains("typedef KPHandle * rl_cptr_"));
    assert!(c.contains("typedef const KPHandle * rl_cptr_"));
    assert!(
        c.contains("const rl_cptr_"),
        "read loans must const-qualify the slot"
    );
    assert!(c.contains("incompatible header prototype for kp_unused"));
    fixture.run_c(&c, Some("42"), "const;word;get;read;", &source);

    // An unused signature still participates in target-header validation.
    fixture.write(
        "k2_native.h",
        &NATIVE_HEADER.replace("kp_unused(uint32_t n)", "kp_unused(int32_t n)"),
    );
    for compiler in compilers() {
        for optimization in ["-O0", "-O2"] {
            let output = fixture.build(&c, compiler, optimization);
            assert!(
                !output.status.success(),
                "unused mismatch accepted by {compiler} {optimization}"
            );
            assert!(String::from_utf8_lossy(&output.stderr)
                .contains("incompatible header prototype for kp_unused"));
        }
    }
}

#[test]
fn logical_native_dependencies_survive_erased_fields_and_untaken_native_paths() {
    let fixture = Fixture::new();
    fixture.write("k2_native.h", NATIVE_HEADER);
    let source = format!("{NATIVE}{FLAG}type Packet<a> = {{ tag: Flag<ptr<KPHandle>>; hidden: Flag<a>; n: i32 }}\nfn make() {{ record Packet {{ tag = Off; hidden = On; n = 42 }} }}\nfn main() {{ let p = make(); if true {{ p.n }} else {{ kp_read(kp_const()) }} }}");
    let program = parser::parse(&source).unwrap();
    let (lowered, c) = lower(&program, &source);
    assert_eq!(typed::evaluate(&lowered).unwrap().to_string(), "42");
    assert_eq!(
        typed::evaluate(&lowered),
        interpreter::run_reference(&program)
    );
    assert_eq!(typed::evaluate(&lowered), interpreter::run(&program));
    assert!(c.contains("incompatible header prototype for kp_unused"));
    fixture.run_c(&c, Some("42"), "", &source);

    // Native logical identity, even without native declarations or physical raw
    // fields, is still outside each closed origin API's native boundary.
    let source = format!("{FLAG}type Packet<a> = {{ tag: Flag<ptr<KPHandle>>; n: i32 }}\nfn get(p: Packet<i32>) {{ p.n }}\nfn main() {{ get(record Packet {{ tag = Off; n = 42 }}) }}");
    let path = fixture.write("logical.rill", &source);
    let sources = load_program_with_sources(&path).unwrap();
    let _ = lower(sources.program(), &source);
    for error in [
        lower_main_with_function_origins(sources.clone()).unwrap_err(),
        lower_main_with_copy_node_origins(sources).unwrap_err(),
    ] {
        assert!(
            error.starts_with("E-TYPED-UNSUPPORTED:") && error.contains("native"),
            "{error}"
        );
    }
}

#[test]
fn product_storage_does_not_reorder_native_argument_errors() {
    let fixture = Fixture::new();
    fixture.write(
        "k2_constant.h",
        "#include <stdint.h>\n#define WORD UINT32_C(40)\n",
    );
    for (source, expected) in [
        ("extern fn consume(n: i32) -> u32;\ntype Packet<a> = Packet(u32)\nfn main() { Packet(consume(1 / 0)) }", "runtime trap"),
        ("type Packet<a> = Packet(u32)\nfn main() { Packet(u32(1 / 0)) }", "E-FFI: raw C values require native C emission"),
        ("@include(\"k2_constant.h\");\nextern const WORD: u32;\ntype Packet<a> = Packet(u32)\nfn main() { Packet(WORD) }", "E-FFI: imported C constants require native C emission"),
    ] {
        let program = parser::parse(source).unwrap();
        let (lowered, _) = lower(&program, source);
        assert_eq!(typed::evaluate(&lowered).unwrap_err(), expected, "{source}");
        assert_eq!(interpreter::run_reference(&program).unwrap_err(), expected, "{source}");
        assert_eq!(interpreter::run(&program).unwrap_err(), expected, "{source}");
    }
}
