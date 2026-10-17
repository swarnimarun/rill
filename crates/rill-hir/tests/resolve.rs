//! Name-resolution tests (PLAN.md §7.1/§7.2, M1).
//!
//! The corpus half reuses `PLAN.md`'s fenced examples and `examples/showcase.rill`,
//! so resolution is exercised on the same programs as the M0 round-trip tests.

use rill_ast::{parse_and_lower_text, Frontend};
use rill_hir::{lower, Hir, ItemKind, LocalKind, RefInfo, Res};
use rill_span::{SourceMap, Symbol};

/// Lowers without asserting that resolution was complete: the plan's examples
/// reference `core`/`std` helpers the module does not declare.
fn resolve_leniently(src: &str) -> (Frontend, Hir, SourceMap) {
    let (frontend, sm) = parse_and_lower_text("resolve.rill", src);
    assert!(
        !frontend.diagnostics.has_errors(),
        "parse failed:\n{}",
        rill_diagnostics::render_all(&sm, &frontend.diagnostics)
    );
    let hir = lower(&frontend);
    (frontend, hir, sm)
}

fn resolve(src: &str) -> (Frontend, Hir, SourceMap) {
    let (frontend, sm) = parse_and_lower_text("resolve.rill", src);
    assert!(
        !frontend.diagnostics.has_errors(),
        "parse failed:\n{}",
        rill_diagnostics::render_all(&sm, &frontend.diagnostics)
    );
    let hir = lower(&frontend);
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

    // Self-recursion inside `quicksort`.
    let quicksort = hir
        .module_items()
        .find(|item| item.name.map(|n| name_of(&frontend, n)) == Some("quicksort"))
        .expect("quicksort item");
    let recursive = refs(&frontend, &hir, "quicksort")
        .into_iter()
        .filter(|r| r.res == Res::Item(quicksort.id))
        .count();
    assert_eq!(recursive, 2, "both recursive calls resolve to the item");

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
    let account = refs(&frontend, &hir, "Account");
    assert_eq!(account.len(), 2);
    for reference in account {
        match reference.res {
            Res::Item(id) => assert!(matches!(hir.item(id).kind, ItemKind::Type(_))),
            ref other => panic!("constructor resolved to {other:?}"),
        }
    }

    // `StackArena::run` resolves through the `use` import.
    let stack_arena = refs(&frontend, &hir, "StackArena");
    assert!(!stack_arena.is_empty());
    for reference in stack_arena {
        match &reference.res {
            Res::Import(path) => {
                let text: Vec<&str> = path.iter().map(|s| name_of(&frontend, *s)).collect();
                assert_eq!(text, vec!["core", "alloc", "StackArena"]);
            }
            other => panic!("import resolved to {other:?}"),
        }
    }

    // Names the module genuinely does not declare stay unresolved: the boundary
    // is explicit instead of silently wrong.
    for name in ["io", "partition"] {
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
    let (frontend, sm) = parse_and_lower_text(
        "scope.rill",
        "let f () = {\n    { inner := 1; inner };\n    inner\n};\n",
    );
    let hir = lower(&frontend);
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
    let (frontend, sm) = parse_and_lower_text(
        "dup.rill",
        "let f (a: i32, a: i32) -> i32 <computes> = a;\nlet g () = missing;\n",
    );
    let hir = lower(&frontend);
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
        vec!["Account", "balance_mut", "transfer", "quicksort", "main"],
        "named module items in declaration order (the two `use`s are anonymous)"
    );
    assert_eq!(
        hir.module_item_ids().len(),
        7,
        "two `use`s plus five named items"
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
    let (frontend, sm) =
        parse_and_lower_text("broken.rill", "let f () = { x := ; };\nlet g () = y;\n");
    let hir = lower(&frontend);
    let parse_rendered = rill_diagnostics::render_all(&sm, &frontend.diagnostics);
    assert!(!parse_rendered.is_empty(), "the parse error is reported");
    let hir_rendered = rill_diagnostics::render_all(&sm, &hir.diagnostics);
    assert!(hir_rendered.contains("unknown name `y`"), "{hir_rendered}");
}
