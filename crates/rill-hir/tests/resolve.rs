//! Name-resolution tests (PLAN.md §7.1/§7.2, M1).
//!
//! The corpus half reuses `PLAN.md`'s fenced examples and `examples/showcase.rill`,
//! so resolution is exercised on the same programs as the M0 round-trip tests.

use rill_ast::{parse_and_lower_text, Frontend};
use rill_hir::{lower, Hir, ItemKind, LocalKind, RefInfo, RefKind, Res};
use rill_span::{SourceMap, Symbol};

/// Lowers without asserting that resolution was complete: the plan's examples
/// reference `core`/`std` helpers the module does not declare.
fn resolve_leniently(src: &str) -> (Frontend, Hir, SourceMap) {
    let (mut frontend, sm) = parse_and_lower_text("resolve.rill", src);
    assert!(
        !frontend.diagnostics.has_errors(),
        "parse failed:\n{}",
        rill_diagnostics::render_all(&sm, &frontend.diagnostics)
    );
    let hir = lower(&mut frontend);
    (frontend, hir, sm)
}

fn resolve(src: &str) -> (Frontend, Hir, SourceMap) {
    let (mut frontend, sm) = parse_and_lower_text("resolve.rill", src);
    assert!(
        !frontend.diagnostics.has_errors(),
        "parse failed:\n{}",
        rill_diagnostics::render_all(&sm, &frontend.diagnostics)
    );
    let hir = lower(&mut frontend);
    assert!(
        !hir.diagnostics.has_errors(),
        "resolution failed:\n{}",
        rill_diagnostics::render_all(&sm, &hir.diagnostics)
    );
    (frontend, hir, sm)
}

fn name_of(frontend: &Frontend, sym: Symbol) -> &str {
    frontend.interner.get(sym)
}

/// Every reference whose recorded head name is `name`.
fn refs<'a>(frontend: &'a Frontend, hir: &'a Hir, name: &str) -> Vec<&'a RefInfo> {
    hir.refs()
        .iter()
        .filter(|r| name_of(frontend, r.name) == name)
        .collect()
}

/// Names declared anywhere in the module: items, members, locals and imports.
fn declared_names(frontend: &Frontend, hir: &Hir) -> Vec<String> {
    let mut names: Vec<String> = hir
        .items()
        .iter()
        .filter_map(|item| item.name.map(|n| name_of(frontend, n).to_string()))
        .collect();
    names.extend(
        hir.locals()
            .iter()
            .map(|l| name_of(frontend, l.name).to_string()),
    );
    for item in hir.items() {
        if let ItemKind::Use(use_def) = &item.kind {
            names.extend(
                use_def
                    .names
                    .iter()
                    .map(|n| name_of(frontend, *n).to_string()),
            );
        }
    }
    names
}

const PLAN: &str = include_str!("../../../PLAN.md");
const SHOWCASE: &str = include_str!("../../../examples/showcase.rill");

fn plan_rill_fences() -> Vec<String> {
    let mut fences = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in PLAN.lines() {
        let trimmed = line.trim_start();
        match current.as_mut() {
            None if trimmed.starts_with("```rill") => current = Some(Vec::new()),
            Some(_) if trimmed.starts_with("```") => {
                let lines = current.take().expect("borrowed above");
                fences.push(format!("{}\n", lines.join("\n")));
            }
            Some(lines) => lines.push(line),
            None => {}
        }
    }
    fences
}

#[test]
fn corpus_resolves_declared_names() {
    let mut sources = plan_rill_fences();
    sources.push(SHOWCASE.to_string());
    assert!(sources.len() >= 11, "the plan's examples plus the showcase");

    for source in &sources {
        let (frontend, hir, sm) = resolve_leniently(source);
        let declared = declared_names(&frontend, &hir);
        // Every diagnostic in the corpus is an unresolved external name;
        // fields, locals and declared items all resolve cleanly.
        for diagnostic in hir.diagnostics.items() {
            assert!(
                diagnostic.message.contains("unknown name"),
                "unexpected resolution diagnostic in:\n{source}\n{}",
                rill_diagnostics::render_all(&sm, &hir.diagnostics)
            );
        }
        for reference in hir.refs() {
            if reference.res.is_error() {
                // The examples reference `core`/`std` helpers and `io::…` which
                // no module declares; what must never happen is a *declared*
                // name failing to resolve.
                let head = name_of(&frontend, reference.name).to_string();
                assert!(
                    !declared.contains(&head),
                    "declared name `{head}` failed to resolve in:\n{source}"
                );
            }
        }
    }
}

#[test]
fn showcase_references_resolve_to_their_targets() {
    // The showcase calls `core`/`std` helpers (`partition`, `io::println`, …)
    // that no module declares, so resolution is expected to be incomplete.
    let (frontend, hir, _sm) = resolve_leniently(SHOWCASE);

    // `transfer` in `main` resolves to the `transfer` function item.
    let transfer_item = hir
        .module_items()
        .find(|item| item.name.map(|n| name_of(&frontend, n)) == Some("transfer"))
        .expect("transfer item");
    assert!(matches!(transfer_item.kind, ItemKind::Fn(_)));
    let transfer_refs = refs(&frontend, &hir, "transfer");
    assert_eq!(
        transfer_refs.len(),
        2,
        "two calls inside the `option` handler"
    );
    for reference in transfer_refs {
        assert_eq!(reference.res, Res::Item(transfer_item.id));
    }

    // `first_positive` is referenced from `main` six times: twice to build the
    // option values, three times in the `all` chain's arms, and once in `sync`.
    let first_positive = hir
        .module_items()
        .find(|item| item.name.map(|n| name_of(&frontend, n)) == Some("first_positive"))
        .expect("first_positive item");
    let referenced = refs(&frontend, &hir, "first_positive")
        .into_iter()
        .filter(|r| r.res == Res::Item(first_positive.id))
        .count();
    assert_eq!(referenced, 6, "every call resolves to the item");

    // `alice`, `bob` and `scratch` are mutable locals of `main`.
    for name in ["alice", "bob", "scratch"] {
        let found = refs(&frontend, &hir, name);
        assert!(!found.is_empty(), "`{name}` is referenced");
        for reference in found {
            match reference.res {
                Res::Local(id) => {
                    let local = hir.local(id);
                    assert_eq!(name_of(&frontend, local.name), name);
                    assert_eq!(local.kind, LocalKind::Var);
                }
                ref other => panic!("`{name}` resolved to {other:?}"),
            }
        }
    }

    // The projection `balance_mut` is an item, reached as `from . balance_mut`.
    let balance_mut = refs(&frontend, &hir, "balance_mut");
    assert_eq!(balance_mut.len(), 2);
    for reference in balance_mut {
        match reference.res {
            Res::Item(id) => assert!(matches!(hir.item(id).kind, ItemKind::Project(_))),
            ref other => panic!("projection resolved to {other:?}"),
        }
    }

    // `Account 1u64 500i64` is a constructor call: a type name used as a value.
    // The same name in type position (`from: Account`) is recorded too, with
    // the kind telling the two apart.
    let account_exprs = refs(&frontend, &hir, "Account")
        .into_iter()
        .filter(|r| r.kind == RefKind::Expr)
        .collect::<Vec<_>>();
    assert_eq!(account_exprs.len(), 2);
    for reference in account_exprs {
        match reference.res {
            Res::Item(id) => assert!(matches!(hir.item(id).kind, ItemKind::Type(_))),
            ref other => panic!("constructor resolved to {other:?}"),
        }
    }
    let account_types = refs(&frontend, &hir, "Account")
        .into_iter()
        .filter(|r| r.kind == RefKind::Type)
        .collect::<Vec<_>>();
    assert!(
        !account_types.is_empty(),
        "`Account` is written in type position"
    );
    for reference in account_types {
        match reference.res {
            Res::Item(id) => assert!(matches!(hir.item(id).kind, ItemKind::Type(_))),
            ref other => panic!("type reference resolved to {other:?}"),
        }
    }

    // `io::println` resolves through the `use std::io` import.
    let io = refs(&frontend, &hir, "io");
    assert!(!io.is_empty());
    for reference in io {
        match &reference.res {
            Res::Import(path) => {
                let text: Vec<&str> = path.iter().map(|s| name_of(&frontend, *s)).collect();
                assert_eq!(text, vec!["std", "io"]);
            }
            other => panic!("import resolved to {other:?}"),
        }
    }

    // Names the module genuinely does not declare stay unresolved: the boundary
    // is explicit instead of silently wrong.
    for name in ["partition", "quicksort"] {
        for reference in refs(&frontend, &hir, name) {
            assert!(
                reference.res.is_error(),
                "`{name}` is not declared by the module"
            );
        }
    }
}

#[test]
fn scopes_shadow_and_expire() {
    let src = "\
let f (x: i32) -> i32 <computes> = {
    y := {
        x := 1;
        x
    };
    x + y
};
";
    let (frontend, hir, sm) = resolve(src);
    let x_refs = refs(&frontend, &hir, "x");
    assert_eq!(
        x_refs.len(),
        2,
        "the inner block's use and the outer use: {x_refs:?}"
    );
    assert_ne!(
        x_refs[0].res, x_refs[1].res,
        "the shadowing binding is a different local than the parameter"
    );
    let kinds: Vec<LocalKind> = x_refs
        .iter()
        .map(|r| match r.res {
            Res::Local(id) => hir.local(id).kind,
            ref other => panic!("`x` resolved to {other:?}"),
        })
        .collect();
    assert!(
        kinds.contains(&LocalKind::Param),
        "the outer `x` is the parameter: {kinds:?}"
    );
    assert!(
        !hir.diagnostics.has_errors(),
        "{}",
        rill_diagnostics::render_all(&sm, &hir.diagnostics)
    );
}

#[test]
fn a_local_of_a_nested_block_is_invisible_outside() {
    let (mut frontend, sm) = parse_and_lower_text(
        "scope.rill",
        "let f () = {\n    { inner := 1; inner };\n    inner\n};\n",
    );
    let hir = lower(&mut frontend);
    let rendered = rill_diagnostics::render_all(&sm, &hir.diagnostics);
    assert!(rendered.contains("unknown name `inner`"), "{rendered}");
    // The in-block use still resolves; only the outer one is unknown.
    let inner_refs = refs(&frontend, &hir, "inner");
    assert_eq!(inner_refs.len(), 2);
    assert!(matches!(inner_refs[0].res, Res::Local(_)));
    assert!(inner_refs[1].res.is_error());
}

#[test]
fn unknown_and_duplicate_names_are_reported() {
    let (mut frontend, sm) = parse_and_lower_text(
        "dup.rill",
        "let f (a: i32, a: i32) -> i32 <computes> = a;\nlet g () = missing;\n",
    );
    let hir = lower(&mut frontend);
    let rendered = rill_diagnostics::render_all(&sm, &hir.diagnostics);
    assert!(rendered.contains("`a` is already bound"), "{rendered}");
    assert!(rendered.contains("unknown name `missing`"), "{rendered}");
    // The duplicate binds once, so the body still resolves to a local.
    let a_refs = refs(&frontend, &hir, "a");
    assert!(
        a_refs.iter().all(|r| matches!(r.res, Res::Local(_))),
        "{a_refs:?}"
    );
}

#[test]
fn pattern_and_loop_bindings_are_scoped() {
    let src = "\
type Pair = record { a: i32; b: i32; };

let classify (p: Pair) -> i32 <computes> = match p {
    Pair (a, b) -> a + b,
    _ -> 0
};

let total (xs: Array i32) -> i32 <computes> = {
    var sum = 0;
    for (item := xs) {
        sum := item
    };
    sum
};
";
    let (frontend, hir, sm) = resolve(src);
    for name in ["a", "b", "item", "sum"] {
        let found = refs(&frontend, &hir, name);
        assert!(!found.is_empty(), "`{name}` is referenced");
        for reference in found {
            assert!(
                matches!(reference.res, Res::Local(_)),
                "`{name}` resolved to {:?}",
                reference.res
            );
        }
    }
    let kinds: Vec<LocalKind> = hir.locals().iter().map(|l| l.kind).collect();
    assert!(
        kinds.contains(&LocalKind::Pattern),
        "pattern bindings are locals"
    );
    assert!(kinds.contains(&LocalKind::Var), "`var` bindings are locals");
    assert!(
        !hir.diagnostics.has_errors(),
        "{}",
        rill_diagnostics::render_all(&sm, &hir.diagnostics)
    );
}

#[test]
fn block_level_lets_are_items_in_their_scope() {
    let src = "\
let outer () = {
    let helper (n: i32) -> i32 <computes> = helper n;
    helper 1
};
";
    let (frontend, hir, sm) = resolve(src);
    let helper_refs = refs(&frontend, &hir, "helper");
    assert_eq!(
        helper_refs.len(),
        2,
        "recursive call plus the use: {helper_refs:?}"
    );
    for reference in helper_refs {
        match reference.res {
            Res::Item(id) => assert!(matches!(hir.item(id).kind, ItemKind::Fn(_))),
            ref other => panic!("`helper` resolved to {other:?}"),
        }
    }
    assert!(
        !hir.diagnostics.has_errors(),
        "{}",
        rill_diagnostics::render_all(&sm, &hir.diagnostics)
    );
}

#[test]
fn builder_names_resolve_as_values() {
    let src = "\
builder seq = {
    let ret (u: ()) -> Step a <computes> = Step::Done;
};

let evens (n: i32) = seq {
    for (i : 0..n) { yield i }
};
";
    // `Step` is an undeclared external in this fixture, so resolve leniently.
    let (frontend, hir, _sm) = resolve_leniently(src);
    let seq_refs = refs(&frontend, &hir, "seq");
    assert!(!seq_refs.is_empty(), "the builder is applied");
    for reference in seq_refs {
        match reference.res {
            Res::Item(id) => assert!(matches!(hir.item(id).kind, ItemKind::Builder(_))),
            ref other => panic!("builder resolved to {other:?}"),
        }
    }
}

#[test]
fn hir_keeps_item_and_local_tables_consistent() {
    let (frontend, hir, _sm) = resolve_leniently(SHOWCASE);
    let module_items: Vec<&str> = hir
        .module_items()
        .filter_map(|item| item.name.map(|n| name_of(&frontend, n)))
        .collect();
    assert_eq!(
        module_items,
        vec![
            "Account",
            "balance_mut",
            "transfer",
            // The showcase grew two types, a trait, two impls and three tasks.
            // Read this list off the artifact, never from memory.
            "Point",
            "Box",
            "Shape",
            "Shape",
            "Shape",
            "first_positive",
            "tick_one",
            "tick_two",
            "tick_three",
            "main"
        ],
        "named module items in declaration order (the two `use`s are anonymous)"
    );
    assert_eq!(
        hir.module_item_ids().len(),
        14,
        "one `use` plus five named items"
    );

    for local in hir.locals() {
        assert!(local.owner.is_some(), "every local belongs to an item");
    }
    for item in hir.items() {
        if let ItemKind::Trait(trait_def) = &item.kind {
            for member in &trait_def.members {
                assert!(
                    hir.try_item(*member).is_some(),
                    "member ids are in the item table"
                );
            }
        }
    }
    assert!(
        hir.items()
            .iter()
            .all(|item| !matches!(item.kind, ItemKind::Placeholder)),
        "the declaration pass leaves no unfinished slots"
    );
}

#[test]
fn malformed_input_still_resolves() {
    let (mut frontend, sm) =
        parse_and_lower_text("broken.rill", "let f () = { x := ; };\nlet g () = y;\n");
    let hir = lower(&mut frontend);
    let parse_rendered = rill_diagnostics::render_all(&sm, &frontend.diagnostics);
    assert!(!parse_rendered.is_empty(), "the parse error is reported");
    let hir_rendered = rill_diagnostics::render_all(&sm, &hir.diagnostics);
    assert!(hir_rendered.contains("unknown name `y`"), "{hir_rendered}");
}

#[test]
fn doc_comments_attach_to_the_declarations_they_precede() {
    let src = "\
/// Adds two numbers.
///
/// The second paragraph.
let add (a: i32, b: i32) -> i32 <computes> = a + b;
//// Four slashes are an ordinary comment.
let plain () -> i32 <computes> = 1;
/// A point.
type Point = record {
    /// The x coordinate.
    var x: i32;
    /// The y coordinate.
    var y: i32;
};
/// A step.
type Step =
    /// One turn.
    | Yield i32
    /// The end.
    | Done;
";
    let (frontend, hir, _sm) = resolve(src);
    let item = |name: &str| {
        hir.items()
            .iter()
            .find(|item| item.name.is_some_and(|n| name_of(&frontend, n) == name))
            .unwrap_or_else(|| panic!("no item {name}"))
    };
    assert_eq!(
        item("add").docs,
        ["Adds two numbers.", "", "The second paragraph."]
    );
    assert!(item("plain").docs.is_empty());

    // Field and variant docs are part of the AST that HIR reuses for type
    // syntax, so they are checked on the lowered module.
    let type_decl = |name: &str| {
        frontend
            .module
            .items
            .iter()
            .find_map(|item| match item {
                rill_ast::Item::Type(decl) if frontend.text(decl.name) == name => Some(decl),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no type {name}"))
    };
    let point = type_decl("Point");
    let rill_ast::TypeBody::Record(record) = &point.body else {
        panic!("Point is not a record");
    };
    assert_eq!(record.fields[0].docs, ["The x coordinate."]);
    assert_eq!(record.fields[1].docs, ["The y coordinate."]);
    let step = type_decl("Step");
    let rill_ast::TypeBody::Variants(variants) = &step.body else {
        panic!("Step is not an enum");
    };
    assert_eq!(variants[0].docs, ["One turn."]);
    assert_eq!(variants[1].docs, ["The end."]);
}

#[test]
fn module_statics_resolve_as_locals_in_any_order() {
    let src = "\
let peek () -> i32 <computes> = counter;
static var counter: i32 = 0;
let bump () -> i32 <computes> = {
    set counter = counter + 1;
    counter
};
static threadlocal var scratch: i32 = 5;
";
    let (frontend, hir, _sm) = resolve(src);
    let counter = hir
        .locals()
        .iter()
        .find(|local| name_of(&frontend, local.name) == "counter")
        .expect("the module binding is a local");
    let uses = refs(&frontend, &hir, "counter");
    assert_eq!(uses.len(), 4, "every use is recorded: {uses:?}");
    assert!(
        uses.iter()
            .all(|reference| reference.res == Res::Local(counter.id)),
        "a forward reference names the same local: {uses:?}"
    );

    let storage = |name: &str| {
        let id = hir
            .locals()
            .iter()
            .find(|local| name_of(&frontend, local.name) == name)
            .unwrap_or_else(|| panic!("no local {name}"))
            .id;
        hir.items()
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Stmt(rill_hir::Stmt::Var { storage, local, .. }) if *local == id => {
                    Some(*storage)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no statement for {name}"))
    };
    assert_eq!(storage("counter"), rill_ast::VarStorage::Static);
    assert_eq!(storage("scratch"), rill_ast::VarStorage::ThreadLocal);
}

#[test]
fn global_var_and_block_static_are_rejected() {
    let (mut frontend, sm) = parse_and_lower_text(
        "globals.rill",
        "var counter: i32 = 0;\n\
         let main () -> i32 <computes> = { static var local = 1; local };\n",
    );
    let hir = lower(&mut frontend);
    let rendered = rill_diagnostics::render_all(&sm, &hir.diagnostics);
    assert!(
        rendered.contains("a module-level `var` must be declared `static var`"),
        "{rendered}"
    );
    assert!(
        rendered.contains("`static` storage is only allowed at module level"),
        "{rendered}"
    );
}
