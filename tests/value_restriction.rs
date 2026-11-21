use rill::{ast::Expr, check, parser};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

const OPTION: &str = "type Option<a> = None | Some(a)\nfn integer(x: Option<i32>) { match x { None => 40 | Some(n) => n } }\nfn boolean(x: Option<bool>) { match x { None => true | Some(b) => b } }\n";
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn accepts(source: &str) {
    let program = parser::parse(source).unwrap();
    check::check(&program).unwrap_or_else(|error| panic!("{source}: {error}"));
}
fn rejects_type(source: &str) {
    let program = parser::parse(source).unwrap();
    let error = check::check(&program).unwrap_err();
    assert!(error.starts_with("E-TYPE:"), "{source}: {error}");
}

#[test]
fn nullary_constructors_and_immutable_aliases_generalize_independently() {
    for bindings in [
        "let n = None;",
        "let first = None; let second = first; let n = second;",
        "let n = None();",
    ] {
        accepts(&format!(
            "{OPTION}fn main() {{ {bindings} if boolean(n) {{ integer(n) + 2 }} else {{ 0 }} }}"
        ));
    }
}

#[test]
fn constructor_and_record_values_generalize_only_independent_variables() {
    accepts("type Option<a> = None | Some(a)\nfn integer(x: Option<Option<i32>>) { 40 }\nfn boolean(x: Option<Option<bool>>) { 2 }\nfn main() { let empty = None; let n = Some(empty); let alias = n; integer(alias) + boolean(alias) }");
    accepts("type Option<a> = None | Some(a)\ntype Holder<a> = { value: Option<a> }\nfn integer(x: Holder<i32>) { 40 }\nfn boolean(x: Holder<bool>) { 2 }\nfn main() { let n = record Holder { value = None }; let alias = n; integer(alias) + boolean(alias) }");
    rejects_type("type Option<a> = None | Some(a)\ntype Holder<a> = { value: Option<a> }\nfn integer(x: Holder<i32>) { 40 }\nfn boolean(x: Holder<bool>) { 2 }\nfn absent() { None }\nfn main() { let n = record Holder { value = absent() }; integer(n) + boolean(n) }");
    rejects_type("type Option<a> = None | Some(a)\nfn integer(x: Option<Option<i32>>) { 40 }\nfn boolean(x: Option<Option<bool>>) { 2 }\nfn absent() { None }\nfn main() { let n = Some(absent()); integer(n) + boolean(n) }");
    // The parameter remains one type while the unrelated None may generalize.
    accepts("type Option<a> = None | Some(a)\ntype Pair<a,b> = Pair(a,b)\nfn integer(x: Pair<i32,Option<i32>>) { 40 }\nfn boolean(x: Pair<i32,Option<bool>>) { 2 }\nfn use(x) { let n = Pair(x,None); integer(n) + boolean(n) }\nfn main() { use(0) }");
    rejects_type("type Option<a> = None | Some(a)\ntype Pair<a,b> = Pair(a,b)\nfn integer(x: Pair<i32,Option<i32>>) { 40 }\nfn boolean(x: Pair<bool,Option<bool>>) { 2 }\nfn bad(x) { let n = Pair(x,None); integer(n) + boolean(n) }\nfn main() { 0 }");
}

#[test]
fn function_condition_and_match_results_are_monomorphic() {
    for initializer in [
        "make_none()",
        "if true { None } else { None }",
        "match true { true => None | false => None }",
        "{ let unused = 0; None }",
        "{ while false { () }; None }",
        "{ let mut slot = 0; slot <- 1; None }",
    ] {
        rejects_type(&format!("{OPTION}fn make_none() {{ None }}\nfn main() {{ let n = {initializer}; if boolean(n) {{ integer(n) }} else {{ 0 }} }}"));
        accepts(&format!(
            "{OPTION}fn make_none() {{ None }}\nfn main() {{ let n = {initializer}; integer(n) }}"
        ));
    }
    // Binary expressions remain excluded even when the only fresh variable is phantom.
    rejects_type("type Phantom<a> = Mark(i32)\nfn integer(x: Phantom<i32>) { 40 }\nfn boolean(x: Phantom<bool>) { 2 }\nfn main() { let n = Mark(1 + 1); integer(n) + boolean(n) }");
}

#[test]
fn alias_chains_cannot_regeneralize_expansive_variables() {
    for initializer in [
        "make_none()",
        "if true { None } else { None }",
        "match true { _ => None }",
    ] {
        rejects_type(&format!("{OPTION}fn make_none() {{ None }}\nfn main() {{ let first = {initializer}; let second = first; let third = second; let n = third; if boolean(n) {{ integer(first) }} else {{ 0 }} }}"));
        rejects_type(&format!("{OPTION}fn make_none() {{ None }}\nfn main() {{ let first = {{ let hidden = {initializer}; hidden }}; let n = first; if boolean(n) {{ integer(first) }} else {{ 0 }} }}"));
    }
    rejects_type("type Option<a> = None | Some(a)\ntype Wrap<a> = Wrap(Option<a>)\nfn make_none() { None }\nfn integer(x: Wrap<i32>) { 40 }\nfn boolean(x: Wrap<bool>) { 2 }\nfn main() { let first = make_none(); let n = Wrap(first); integer(n) + boolean(n) }");
}

#[test]
fn mutable_outer_and_read_dependencies_remain_monomorphic() {
    for bindings in [
        "let mut slot = None; let n = slot;",
        "let mut slot = None; let first = slot; let n = first;",
    ] {
        rejects_type(&format!(
            "{OPTION}fn main() {{ {bindings} if boolean(n) {{ integer(slot) }} else {{ 0 }} }}"
        ));
    }
    for parameter in ["x", "read x", "inout x"] {
        rejects_type(&format!("{OPTION}fn bad({parameter}) {{ let n = x; if boolean(n) {{ integer(x) }} else {{ 0 }} }}\nfn main() {{ 0 }}"));
    }
    for (parameter, bindings) in [("read x", ""), ("inout x", ""), ("", "let mut x = 1;")] {
        rejects_type(&format!("type Phantom<a> = Mark(i32)\nfn integer(x: Phantom<i32>) {{ 40 }}\nfn boolean(x: Phantom<bool>) {{ 2 }}\nfn bad({parameter}) {{ {bindings} let n = Mark(x); integer(n) + boolean(n) }}\nfn main() {{ 0 }}"));
    }
}

#[test]
fn field_reads_and_active_loans_do_not_generalize() {
    rejects_type(&format!("{OPTION}type Holder<a> = {{ value: Option<a> }}\nfn main() {{ let n = record Holder {{ value = None }}; let field = n.value; if boolean(field) {{ integer(field) }} else {{ 0 }} }}"));
    rejects_type("type Phantom<a> = Mark(i32)\nfn integer(x: Phantom<i32>) { 40 }\nfn boolean(x: Phantom<bool>) { 2 }\nfn keep(read x, y) { y }\nfn main() { let x = 1; keep(x, { let n = Mark(x); integer(n) + boolean(n) }) }");
    accepts("type Phantom<a> = Mark(i32)\nfn integer(x: Phantom<i32>) { 40 }\nfn boolean(x: Phantom<bool>) { 2 }\nfn main() { let x = 1; let n = Mark(x); integer(n) + boolean(n) }");
}

#[test]
fn projections_instantiate_immutable_record_schemes_without_changing_roots() {
    let declarations = format!("{OPTION}type Holder<a> = {{ value: Option<a> }}\nfn boolean_record(x: Holder<bool>) {{ 2 }}\n");
    accepts(&format!("{declarations}fn main() {{ let r = record Holder {{ value = None }}; let n = r.value; integer(n) + boolean_record(r) }}"));
    accepts(&format!("{declarations}fn main() {{ let r = record Holder {{ value = None }}; let n = r.value; let other = r.value; if boolean(other) {{ integer(n) + 2 }} else {{ 0 }} }}"));
    rejects_type(&format!("{declarations}fn main() {{ let mut r = record Holder {{ value = None }}; let n = r.value; r.value <- Some(1); if boolean(n) {{ 42 }} else {{ 0 }} }}"));
    for mode in ["read", "inout"] {
        rejects_type(&format!("{declarations}fn bad({mode} r) {{ let same = if true {{ r }} else {{ record Holder {{ value = None }} }}; let n = r.value; if boolean(r.value) {{ integer(n) }} else {{ 0 }} }}\nfn main() {{ 0 }}"));
    }
    rejects_type("type A<a> = { value: a }\ntype B<a> = { value: a }\nfn main() { let r = record A { value = 1 }; if true { r } else { record B { value = 1 } } }");
}

#[test]
fn constructor_resolution_respects_shadowing_and_declaration_identity() {
    rejects_type(&format!("{OPTION}fn main() {{ let mut None = Some(1); let n = None; if boolean(n) {{ integer(n) }} else {{ 0 }} }}"));
    rejects_type(&format!("{OPTION}fn Empty() {{ None }}\nfn main() {{ let n = Empty(); if boolean(n) {{ integer(n) }} else {{ 0 }} }}"));
    accepts(&format!("{OPTION}fn main() {{ let None = 7; let n = None(); if boolean(n) {{ integer(n) + 2 }} else {{ 0 }} }}"));
    rejects_type("type First<a> = First(a)\ntype Second<a> = Second(a)\nfn main() { let n = First(1); if true { n } else { Second(1) } }");
}

#[test]
fn top_level_functions_and_recursive_groups_keep_generic_schemes() {
    let program = parser::parse("type Option<a> = None | Some(a)\nfn identity(x) { let alias = x; alias }\nfn absent() { let n = if true { None } else { None }; let alias = n; alias }\nfn first(n, x) { if n == 0 { x } else { second(n - 1, x) } }\nfn second(n, x) { if n == 0 { x } else { first(n - 1, x) } }\nfn main() { let n = first(2, identity(42)); if second(2, identity(true)) { n } else { 0 } }").unwrap();
    let checked = check::analyze(&program).unwrap();
    for name in ["identity", "absent", "first", "second"] {
        assert_eq!(checked.functions[name].quantified.len(), 1, "{name}");
    }
    let absent = program
        .functions
        .iter()
        .find(|f| f.name == "absent")
        .unwrap();
    let Expr::Let(_, initializer, body) = &absent.body else {
        panic!("expected let");
    };
    let Expr::Let(_, alias, _) = body.as_ref() else {
        panic!("expected alias");
    };
    assert_eq!(
        checked.expression_type(initializer),
        checked.expression_type(alias)
    );
}

#[test]
fn owner_dependency_scan_does_not_expand_generic_declaration_trees() {
    let mut source = "type Tree0<a> = Leaf0 | Fork0(a,a)\n".to_string();
    for index in 1..=80 {
        let previous = index - 1;
        source.push_str(&format!(
            "type Tree{index}<a> = Leaf{index} | Fork{index}(Tree{previous}<Tree{previous}<a>>)\n"
        ));
    }
    source.push_str("fn main() { let empty = Leaf80; 42 }");
    accepts(&source);
}

#[test]
fn pointers_allocations_moves_and_owner_dependencies_keep_restrictions() {
    rejects_type("extern fn first(x: ptr<One>) -> unit;\nextern fn second(x: ptr<Two>) -> unit;\nfn main() { let p = c_null(); let alias = p; first(alias); second(p) }");
    rejects_type("type Owned<a> = Empty | Full(SystemBoxI32)\nfn integer(x: Owned<i32>) { 40 }\nfn boolean(x: Owned<bool>) { 2 }\nfn main() { let n = Empty; integer(n) + boolean(n) }");
    rejects_type("type Owned<a> = Empty | Full(SystemBytes)\ntype Wrapper<a> = Wrap(Owned<a>)\nfn integer(x: Wrapper<i32>) { 40 }\nfn boolean(x: Wrapper<bool>) { 2 }\nfn main() { let n = Wrap(Empty); integer(n) + boolean(n) }");
    rejects_type("type Owned<a> = Owned(SystemBoxI32)\nfn integer(x: Owned<i32>) { 40 }\nfn boolean(x: Owned<bool>) { 2 }\nfn main() { let b = system_box_i32_abort(1); let n = Owned(move b); integer(n) + boolean(n) }");
    let program = parser::parse("fn identity(x) { x }\nfn main() { let b = system_box_i32_abort(42); let moved = identity(move b); box_read(moved) }").unwrap();
    assert!(check::check(&program).unwrap_err().contains("generic"));
    accepts("fn main() { let b = system_box_i32_abort(42); let moved = move b; box_read(moved) }");
}

fn compare(source: &str, expected: &str, typed_supported: bool) {
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    let reference = rill::interpreter::run_reference(&program).unwrap();
    assert_eq!(reference.to_string(), expected, "{source}");
    assert_eq!(rill::interpreter::run(&program).unwrap(), reference);
    if typed_supported {
        let typed = rill::typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(rill::typed::evaluate(&typed).unwrap(), reference);
    }
    let directory = std::env::temp_dir().join(format!(
        "rill-value-restriction-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let c = directory.join("main.c");
    let binary = directory.join("main");
    fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
    for optimization in ["-O0", "-O2"] {
        let output = Command::new("cc")
            .args(["-std=c11", "-pedantic-errors", optimization])
            .arg(&c)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, format!("{expected}\n").as_bytes());
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn monomorphic_values_and_migrations_agree_across_execution_paths() {
    compare(&format!("{OPTION}fn make_none() {{ None }}\nfn main() {{ let n = make_none(); integer(n) + 2 }}"), "42", true);
    compare(&format!("{OPTION}fn make_none() {{ None }}\nfn main() {{ let a = make_none(); let b = make_none(); if boolean(b) {{ integer(a) + 2 }} else {{ 0 }} }}"), "42", true);
    compare(
        &format!(
            "{OPTION}fn main() {{ let n = if true {{ None }} else {{ Some(0) }}; integer(n) + 2 }}"
        ),
        "42",
        true,
    );
    compare(&format!("{OPTION}fn main() {{ let n = match true {{ true => None | false => Some(0) }}; integer(n) + 2 }}"), "42", true);
    compare("type Pair = Pair(i32,i32)\nfn next(inout n) { n <- n + 1; n }\nfn main() { let mut n = 0; let p = Pair(next(inout n), next(inout n)); match p { Pair(a,b) => a * 10 + b } }", "12", true);
    compare("type Pair = { first: i32; second: i32 }\nfn next(inout n) { n <- n + 1; n }\nfn main() { let mut n = 0; let p = record Pair { second = next(inout n); first = next(inout n) }; p.first * 10 + p.second }", "21", false);
}

#[test]
fn accepted_polymorphic_local_values_share_checked_typed_execution() {
    let program = parser::parse(&format!(
        "{OPTION}fn main() {{ let n = None; if boolean(n) {{ integer(n) + 2 }} else {{ 0 }} }}"
    ))
    .unwrap();
    let checked = check::analyze(&program).unwrap();
    assert_eq!(
        rill::interpreter::run_reference(&program)
            .unwrap()
            .to_string(),
        "42"
    );
    let typed = rill::typed::lower_main(&checked).unwrap();
    assert_eq!(
        rill::typed::evaluate(&typed),
        rill::interpreter::run_reference(&program)
    );
    assert_eq!(
        rill::typed::emit_c(&typed).unwrap(),
        rill::backend::emit(&program).unwrap()
    );
}

#[test]
fn imported_constructor_identities_qualify_without_spelling_heuristics() {
    let directory = std::env::temp_dir().join(format!(
        "rill-value-modules-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    for module in ["a", "b"] {
        fs::write(directory.join(format!("{module}.rill")), "export type Option<a> = None | Some(a)\nexport fn integer(x: Option<i32>) { 40 }\nexport fn boolean(x: Option<bool>) { 2 }").unwrap();
    }
    let entry = directory.join("main.rill");
    fs::write(
        &entry,
        "import a; import b;\nfn main() { let n = a.None; a.integer(n) + a.boolean(n) }",
    )
    .unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    check::check(&program).unwrap();
    fs::write(
        &entry,
        "import a; import b;\nfn main() { let n = a.None; a.integer(n) + b.boolean(n) }",
    )
    .unwrap();
    let error = rill::modules::compile::load_program(&entry).unwrap_err();
    assert!(error.to_string().contains("E-TYPE:"), "{error}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn copied_storage_aliases_preserve_monotypes_but_allow_new_phantoms() {
    let prefix = "type Option<a> = None | Some(a)\ntype Phantom<a,b> = Mark(Option<a>)\nfn integer(x: Phantom<i32,i32>) { 40 }\nfn boolean(x: Phantom<i32,bool>) { 2 }\nfn different(x: Phantom<bool,bool>) { 2 }\n";
    for (parameter, bindings) in [("read x", ""), ("inout x", ""), ("", "let mut x = None;")] {
        accepts(&format!("{prefix}fn use({parameter}) {{ {bindings} let alias = x; let n = Mark(alias); integer(n) + boolean(n) }}\nfn main() {{ 0 }}"));
        rejects_type(&format!("{prefix}fn bad({parameter}) {{ {bindings} let alias = x; let n = Mark(alias); integer(n) + different(n) }}\nfn main() {{ 0 }}"));
        rejects_type(&format!("{prefix}fn bad({parameter}) {{ {bindings} let alias = x; let n = Mark(alias); integer(n); let incompatible = if true {{ x }} else {{ Some(true) }}; 0 }}\nfn main() {{ 0 }}"));
    }
}

#[test]
fn active_loan_shadowing_distinguishes_local_identity() {
    accepts("type Phantom<a> = Mark(i32)\nfn integer(x: Phantom<i32>) { 40 }\nfn boolean(x: Phantom<bool>) { 2 }\nfn keep(read x,y) { y }\nfn main() { let x = 1; keep(x, { let x = 2; let n = Mark(x); integer(n) + boolean(n) }) }");
    accepts("type Phantom<a> = Mark(i32)\nfn integer(x: Phantom<i32>) { 40 }\nfn boolean(x: Phantom<bool>) { 2 }\nfn keep(inout x,y) { y }\nfn main() { let mut x = 1; keep(inout x, { let x = 2; let n = Mark(x); integer(n) + boolean(n) }) }");
}

#[test]
fn concrete_owners_in_nested_phantom_arguments_prevent_generalization() {
    rejects_type("type Phantom<x,a> = Tag(i32)\ntype Outer<a> = Outer(Phantom<SystemBoxI32,a>)\nfn integer(x: Outer<i32>) { 40 }\nfn boolean(x: Outer<bool>) { 2 }\nfn main() { let n = Outer(Tag(1)); integer(n) + boolean(n) }");
}
