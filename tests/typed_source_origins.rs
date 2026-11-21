//! Function-instance provenance only: these tests do not assert full typed-node,
//! capture/storage, cleanup, generated-C range, or cross-edit source mapping.
use rill::{
    ast::{Expr, ParamMode},
    modules::{compile::load_program_with_sources, provenance::SourceProgram},
    source_map::{AstNode, AstStep},
    typed::provenance::{
        lower_main_with_function_origins, ConcreteType, DefinitionId, FunctionDisposition,
        FunctionInstanceId, FunctionOriginProgram, RequestOrigin,
    },
};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-typed-origins-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let tree = Self(path);
        tree.write("main.rill", source);
        tree
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(self.0.join(name), source).unwrap();
    }
    fn load(&self) -> SourceProgram {
        load_program_with_sources(&self.0.join("main.rill")).unwrap()
    }
    fn lower(&self) -> FunctionOriginProgram {
        lower_main_with_function_origins(self.load()).unwrap()
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        // Only this test's freshly created temporary fixture is removed.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn definition_named(mapped: &FunctionOriginProgram, name: &str) -> DefinitionId {
    let found: Vec<_> = mapped
        .function_definitions()
        .filter(|id| mapped.definition(id).unwrap().symbol.name == name)
        .collect();
    assert_eq!(found.len(), 1, "expected one definition named {name}");
    found[0].clone()
}
fn ordinal(mapped: &FunctionOriginProgram, id: &FunctionInstanceId) -> usize {
    mapped.instances().iter().position(|i| &i.id == id).unwrap()
}
fn type_record(mapped: &FunctionOriginProgram, ty: &ConcreteType) -> String {
    match ty {
        ConcreteType::Nominal {
            definition,
            arguments,
        } => {
            let symbol = mapped.definition(definition).unwrap();
            let module = &mapped.sources().source(symbol.source).unwrap().module;
            format!(
                "{module}::{}<{}>",
                symbol.symbol.name,
                arguments
                    .iter()
                    .map(|a| type_record(mapped, a))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        other => format!("{other:?}"),
    }
}
// Record only structural slots, names and signatures, never domain-token Debug
// output or equality of handles belonging to independent lowering results.
fn stable_records(mapped: &FunctionOriginProgram) -> Vec<String> {
    let mut records = vec![format!("entry {}", ordinal(mapped, &mapped.entry()))];
    for instance in mapped.instances() {
        let definition = mapped.definition(&instance.definition).unwrap();
        records.push(format!(
            "{} {:?} {:?} -> {}",
            definition.symbol.name,
            definition.target,
            instance
                .parameters
                .iter()
                .map(|(mode, ty)| (*mode, type_record(mapped, ty)))
                .collect::<Vec<_>>(),
            type_record(mapped, &instance.result)
        ));
    }
    for request in mapped.requests() {
        let origin = match &request.origin {
            RequestOrigin::Entry => "entry".to_owned(),
            RequestOrigin::Call { caller, site } => format!(
                "{} {:?}",
                ordinal(mapped, caller),
                mapped.source_node(site).unwrap().path
            ),
        };
        records.push(format!("{origin} => {}", ordinal(mapped, &request.callee)));
    }
    records
}
fn verify_requests(mapped: &FunctionOriginProgram) {
    for instance in mapped.instances() {
        let definition = mapped.definition(&instance.definition).unwrap();
        let Some(AstNode::Function(function)) = mapped.sources().node(&definition.target) else {
            panic!("instance is not a source function");
        };
        let modes: Vec<_> = instance.parameters.iter().map(|(mode, _)| *mode).collect();
        assert_eq!(
            modes,
            function.params.iter().map(|p| p.mode).collect::<Vec<_>>()
        );
    }
    let mut entries = 0;
    for request in mapped.requests() {
        let callee = mapped.instance(&request.callee).unwrap();
        match &request.origin {
            RequestOrigin::Entry => {
                entries += 1;
                assert_eq!(request.callee, mapped.entry());
            }
            RequestOrigin::Call { caller, site } => {
                let caller = mapped.instance(caller).unwrap();
                let node = mapped.source_node(site).unwrap();
                assert_eq!(
                    node.path.declaration,
                    mapped
                        .definition(&caller.definition)
                        .unwrap()
                        .target
                        .declaration
                );
                let Some(AstNode::Expression(Expr::Call(name, _))) =
                    mapped.sources().node(&node.path)
                else {
                    panic!("request site is not a source call");
                };
                let definition = mapped.definition(&callee.definition).unwrap();
                let Some(AstNode::Function(function)) = mapped.sources().node(&definition.target)
                else {
                    panic!("callee does not resolve to a source function");
                };
                assert_eq!(name, &function.name);
                for origin in &node.origins {
                    assert!(mapped.sources().origin_text(origin).is_some());
                }
            }
        }
    }
    assert_eq!(entries, 1);
}

#[test]
fn function_instances_include_result_type_and_all_callers() {
    let tree = Tree::new(
        "type Phantom<a> = Marker\n\
         fn make(x) { Marker }\n\
         fn integer(x: Phantom<i32>) { match x { Marker => 20 } }\n\
         fn boolean(x: Phantom<bool>) { match x { Marker => 2 } }\n\
         fn first() { integer(make(0)) }\n\
         fn second() { integer(make(0)) }\n\
         fn unused(x) { x }\n\
         fn main() { first() + second() + boolean(make(0)) }",
    );
    let mapped = tree.lower();
    verify_requests(&mapped);
    let make = definition_named(&mapped, "make");
    let instances: Vec<_> = mapped
        .instances()
        .iter()
        .filter(|i| i.definition == make)
        .collect();
    assert_eq!(instances.len(), 2);
    assert_eq!(
        instances[0].parameters,
        vec![(ParamMode::Value, ConcreteType::Int)]
    );
    assert_eq!(instances[0].parameters, instances[1].parameters);
    assert_ne!(instances[0].result, instances[1].result);
    for instance in &instances {
        let ConcreteType::Nominal {
            definition,
            arguments,
        } = &instance.result
        else {
            panic!("make must retain its nominal result");
        };
        assert_eq!(
            mapped.definition(definition).unwrap().symbol.name,
            "Phantom"
        );
        let callers: HashSet<_> = mapped
            .requests()
            .iter()
            .filter(|r| r.callee == instance.id)
            .map(|r| match &r.origin {
                RequestOrigin::Call { caller, .. } => {
                    let caller = mapped.instance(caller).unwrap();
                    mapped
                        .definition(&caller.definition)
                        .unwrap()
                        .symbol
                        .name
                        .as_str()
                }
                RequestOrigin::Entry => panic!("make is not the entry"),
            })
            .collect();
        match arguments.as_slice() {
            [ConcreteType::Int] => assert_eq!(callers, HashSet::from(["first", "second"])),
            [ConcreteType::Bool] => assert_eq!(callers, HashSet::from(["main"])),
            other => panic!("unexpected return-only arguments: {other:?}"),
        }
    }
    let mut counts: Vec<_> = instances
        .iter()
        .map(|i| {
            mapped
                .requests()
                .iter()
                .filter(|r| r.callee == i.id)
                .count()
        })
        .collect();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, 2]);
    let callers: HashSet<_> = mapped
        .requests()
        .iter()
        .filter(|r| instances.iter().any(|i| i.id == r.callee))
        .map(|r| match &r.origin {
            RequestOrigin::Call { caller, site } => (caller.clone(), site.clone()),
            RequestOrigin::Entry => panic!("make is not the entry"),
        })
        .collect();
    assert_eq!(callers.len(), 3);
    assert!(
        matches!(mapped.function_disposition(&make), Some(FunctionDisposition::Instantiated(ids)) if ids.len() == 2 && ids.iter().all(|id| instances.iter().any(|i| &i.id == id)))
    );
    let unused = definition_named(&mapped, "unused");
    assert!(matches!(
        mapped.function_disposition(&unused),
        Some(FunctionDisposition::NotInstantiated)
    ));
    assert_eq!(mapped.function_definitions().count(), 7);
    assert_eq!(
        rill::typed::evaluate(mapped.typed()).unwrap().to_string(),
        "42"
    );
}

#[test]
fn recursive_requests_are_finite_and_deterministic() {
    let tree = Tree::new(
        "fn main() { let n = first(4, 42); if second(3, true) { n } else { 0 } }\n\
         fn first(n, x) { if n == 0 { x } else { second(n - 1, x) } }\n\
         fn second(n, x) { if n == 0 { x } else { first(n - 1, x) } }",
    );
    let mapped = tree.lower();
    verify_requests(&mapped);
    assert_eq!(mapped.instances().len(), 5);
    assert_eq!(mapped.requests().len(), 7);
    let repeated = tree.lower();
    assert_eq!(stable_records(&mapped), stable_records(&repeated));
    let first = definition_named(&mapped, "first");
    let second = definition_named(&mapped, "second");
    for (from, to) in [(&first, &second), (&second, &first)] {
        assert_eq!(mapped.requests().iter().filter(|r| {
            mapped.instance(&r.callee).unwrap().definition == *to
                && matches!(&r.origin, RequestOrigin::Call { caller, .. } if mapped.instance(caller).unwrap().definition == *from)
        }).count(), 2);
    }
    assert_eq!(
        rill::typed::evaluate(mapped.typed()).unwrap().to_string(),
        "42"
    );
}

#[test]
fn imported_same_names_keep_definition_and_source_identity() {
    let tree = Tree::new(
        "import a as left; import b as right;\nfn main() { left.run(40) + right.run(2) }",
    );
    for module in ["a", "b"] {
        tree.write(&format!("{module}.rill"),
            "type Hidden<a> = Hidden(a)\nfn helper(x: Hidden<i32>) { match x { Hidden(n) => n } }\nexport fn run(x) { helper(Hidden(x)) }");
    }
    let mapped = tree.lower();
    verify_requests(&mapped);
    let helpers: Vec<_> = mapped
        .instances()
        .iter()
        .filter(|i| mapped.definition(&i.definition).unwrap().symbol.name == "helper")
        .collect();
    assert_eq!(helpers.len(), 2);
    assert_ne!(helpers[0].definition, helpers[1].definition);
    let mut modules = HashSet::new();
    let mut nominal_definitions = HashSet::new();
    for helper in helpers {
        let definition = mapped.definition(&helper.definition).unwrap();
        assert!(!definition.symbol.exported);
        let snapshot = mapped.sources().source(definition.source).unwrap();
        modules.insert(snapshot.module.clone());
        let ConcreteType::Nominal {
            definition: nominal,
            arguments,
        } = &helper.parameters[0].1
        else {
            panic!("helper parameter lost its nominal type");
        };
        assert_eq!(arguments, &[ConcreteType::Int]);
        let nominal_symbol = mapped.definition(nominal).unwrap();
        assert_eq!(nominal_symbol.symbol.name, "Hidden");
        assert!(!nominal_symbol.symbol.exported);
        assert_eq!(nominal_symbol.source, definition.source);
        nominal_definitions.insert(nominal.clone());
    }
    assert_eq!(modules, HashSet::from(["a".to_owned(), "b".to_owned()]));
    assert_eq!(nominal_definitions.len(), 2);
    assert_eq!(
        rill::typed::evaluate(mapped.typed()).unwrap().to_string(),
        "42"
    );
    let before = stable_records(&mapped);
    let snapshots = mapped.sources().sources().to_vec();
    tree.write("a.rill", "this is not a valid replacement program");
    fs::remove_file(tree.0.join("b.rill")).unwrap();
    fs::remove_file(tree.0.join("main.rill")).unwrap();
    assert_eq!(mapped.sources().sources(), snapshots);
    assert_eq!(stable_records(&mapped), before);
    verify_requests(&mapped);
}

#[test]
fn foreign_handles_and_changed_clones_cannot_reuse_maps() {
    let tree = Tree::new("fn id(x) { x }\nfn main() { id(42) }");
    let mapped = tree.lower();
    let other = tree.lower();
    let whole_clone = mapped.clone();
    let definition = definition_named(&mapped, "id");
    assert!(whole_clone.definition(&definition).is_some());
    assert!(other.definition(&definition).is_none());
    assert!(other.function_disposition(&definition).is_none());
    for instance in mapped.instances() {
        assert!(whole_clone.instance(&instance.id).is_some());
        assert!(other.instance(&instance.id).is_none());
    }
    for request in mapped.requests() {
        if let RequestOrigin::Call { site, .. } = &request.origin {
            assert!(whole_clone.source_node(site).is_some());
            assert!(other.source_node(site).is_none());
        }
    }
    // Extraction deliberately discards provenance; no public reattachment API
    // is used or assumed. Editing a detached AST cannot alter the paired result.
    let mut detached = mapped.sources().clone().into_program();
    detached
        .functions
        .iter_mut()
        .find(|f| f.name == "main")
        .unwrap()
        .body = Expr::Int(7);
    assert_eq!(rill::interpreter::run(&detached).unwrap().to_string(), "7");
    assert_eq!(
        rill::typed::evaluate(mapped.typed()).unwrap().to_string(),
        "42"
    );
    let detached_typed = whole_clone.into_typed();
    assert_eq!(
        rill::typed::emit_c(&detached_typed),
        rill::typed::emit_c(mapped.typed())
    );
    let moved = mapped;
    assert!(moved.definition(&definition).is_some());
    verify_requests(&moved);
}

#[test]
fn repeated_call_syntax_keeps_distinct_source_nodes() {
    let tree =
        Tree::new("fn id(x) { x }\r\nfn main() {\r\n\tlet a = id(21);\r\n\tid(21) + a\r\n}\r\n");
    let mapped = tree.lower();
    verify_requests(&mapped);
    let sites: Vec<_> = mapped
        .requests()
        .iter()
        .filter_map(|r| match &r.origin {
            RequestOrigin::Call { site, .. } => Some(site),
            RequestOrigin::Entry => None,
        })
        .collect();
    assert_eq!(sites.len(), 2);
    assert_ne!(sites[0], sites[1]);
    let a = mapped.source_node(sites[0]).unwrap();
    let b = mapped.source_node(sites[1]).unwrap();
    assert_ne!(a.path, b.path);
    assert_ne!(a.origins[0].span, b.origins[0].span);
}

#[test]
fn calls_keep_paths_through_records_branches_loops_and_assignments() {
    let tree = Tree::new(
        "type Pair = { left: i32; right: i32 }\n\
         type Maybe = None | Some(i32)\n\
         fn id(x) { x }\n\
         fn main() {\n\
             let mut p = record Pair { right = id(1); left = id(40) };\n\
             p.left <- id(p.left + 1);\n\
             let mut n = 0;\n\
             while id(n < 1) { n <- id(n + 1); };\n\
             id(0);\n\
             if id(true) {\n\
                 match Some(id(p.left)) { Some(x) => id(x + n) | None => id(0) }\n\
             } else { id(0) }\n\
         }",
    );
    let mapped = tree.lower();
    verify_requests(&mapped);
    let sites = mapped
        .requests()
        .iter()
        .filter_map(|r| match &r.origin {
            RequestOrigin::Call { site, .. } => Some(mapped.source_node(site).unwrap()),
            RequestOrigin::Entry => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(sites.len(), 11);
    let observed = sites.iter().map(|n| &n.path).collect::<HashSet<_>>();
    let id = definition_named(&mapped, "id");
    let Some(AstNode::Function(id_function)) = mapped
        .sources()
        .node(&mapped.definition(&id).unwrap().target)
    else {
        panic!("id must retain its resolved function declaration");
    };
    let expected = mapped
        .sources()
        .nodes()
        .iter()
        .filter(|node| {
            matches!(mapped.sources().node(&node.path), Some(AstNode::Expression(Expr::Call(name, _))) if name == &id_function.name)
        })
        .map(|node| &node.path)
        .collect::<HashSet<_>>();
    assert_eq!(observed, expected);
    for step in [
        AstStep::LetValue,
        AstStep::LetBody,
        AstStep::RecordField(0),
        AstStep::RecordField(1),
        AstStep::AssignValue,
        AstStep::Condition,
        AstStep::WhileBody,
        AstStep::Then,
        AstStep::Else,
        AstStep::MatchValue,
        AstStep::Argument(0),
        AstStep::Arm(0),
        AstStep::Arm(1),
        AstStep::ArmBody,
    ] {
        assert!(
            sites.iter().any(|node| node.path.steps.contains(&step)),
            "{step:?}"
        );
    }
    assert_eq!(
        rill::typed::evaluate(mapped.typed()).unwrap().to_string(),
        "42"
    );
    let checked = rill::check::analyze(mapped.sources().program()).unwrap();
    let ordinary = rill::typed::lower_main(&checked).unwrap();
    assert_eq!(
        rill::typed::emit_c(mapped.typed()),
        rill::typed::emit_c(&ordinary)
    );
}

#[test]
fn ordinary_evaluation_and_c_bytes_are_unchanged() {
    for source in [
        "fn id(x) { x }\nfn main() { if id(true) { id(42) } else { 0 } }",
        "type Mark<a> = Mark(i32)\nfn ints(x: Mark<i32>) { match x { Mark(n) => n } }\nfn bools(x: Mark<bool>) { match x { Mark(n) => n } }\nfn main() { let mut count = 0; let snapshot = { count <- count + 1; count }; let p = Mark(snapshot); let alias = p; count <- 20; ints(alias) + bools(p) + count * 2 }",
        "fn bump(inout x) { x <- x + 1 }\nfn forward(inout x) { bump(inout x) }\nfn main() { let mut n = 41; forward(inout n); n }",
        "fn sum(read a, read b) { a + b }\nfn main() { let mut n = 0; sum({ n <- n + 1; n }, { n <- n + 1; n }) + 39 }",
        "fn keep(owner) { let n = box_read(owner); move owner }\nfn main() { let owner = system_box_i32_abort(42); let returned = keep(move owner); box_read(returned) }",
        "fn id(x) { x }\nfn main() { let b = bytes_new(2); bytes_get(id(bytes_set(b, 1, 42)), 1) }",
        "fn id(x) { x }\nfn main() { byte_len(id(\"é\")) + 40 }",
    ] {
        let tree = Tree::new(source);
        let sources = tree.load();
        let checked = rill::check::analyze(sources.program()).unwrap();
        let ordinary = rill::typed::lower_main(&checked).unwrap();
        let c = rill::typed::emit_c(&ordinary).unwrap();
        let expected = rill::typed::evaluate(&ordinary);
        assert_eq!(expected.as_ref().unwrap().to_string(), "42", "{source}");
        assert_eq!(rill::backend::emit(sources.program()).unwrap(), c);
        let mapped = lower_main_with_function_origins(sources).unwrap();
        verify_requests(&mapped);
        assert_eq!(rill::typed::evaluate(mapped.typed()), expected, "{source}");
        assert_eq!(rill::typed::emit_c(mapped.typed()).unwrap(), c, "{source}");
    }
}

#[test]
fn native_and_fallback_paths_keep_their_boundary() {
    for source in [
        "extern fn abs(x: i32) -> i32;\nfn main() { abs(-42) }",
        "extern fn native_text(text: cstr) -> i32;\nfn main() { native_text(c\"literal\") }",
        "fn main() { u32(0) }",
        "fn inspect(p: ptr<void>) { is_null(p) }\nfn main() { inspect(c_null()) }",
    ] {
        let tree = Tree::new(source);
        let sources = tree.load();
        let checked = rill::check::analyze(sources.program()).unwrap();
        let ordinary = rill::typed::lower_main(&checked);
        let before = rill::backend::emit(sources.program()).unwrap();
        assert!(!before.is_empty());
        let mapped_error = lower_main_with_function_origins(sources.clone()).unwrap_err();
        match ordinary {
            Err(ordinary_error) => {
                assert!(ordinary_error.starts_with("E-TYPED-UNSUPPORTED:"));
                assert_eq!(mapped_error, ordinary_error);
            }
            // Native typed support may grow without extending this index's
            // pure-Rill coverage or changing the ordinary backend's output.
            Ok(typed) => {
                assert!(mapped_error.starts_with("E-TYPED-UNSUPPORTED:"));
                assert_eq!(rill::typed::emit_c(&typed).unwrap(), before);
            }
        }
        assert_eq!(rill::backend::emit(sources.program()).unwrap(), before);
    }
}

#[test]
fn raw_native_layout_metadata_stays_outside_function_origins() {
    for raw in ["u8", "u32", "u64", "f32", "ptr<void>", "const_ptr<void>"] {
        for source in [
            format!("type E = Empty | Has({raw})\nfn main() {{ Empty }}"),
            format!("type Marker<a> = Marker\nfn take(x: Marker<{raw}>) {{ 42 }}\nfn main() {{ take(Marker) }}"),
        ] {
            let tree = Tree::new(&source);
            let sources = tree.load();
            let checked = rill::check::analyze(sources.program()).unwrap();
            assert!(!checked.has_c_values(), "{source}");
            let typed = rill::typed::lower_main(&checked).unwrap();
            let before = rill::backend::emit(sources.program()).unwrap();
            assert_eq!(rill::typed::emit_c(&typed).unwrap(), before);
            let error = lower_main_with_function_origins(sources.clone()).unwrap_err();
            assert!(
                error.starts_with("E-TYPED-UNSUPPORTED: function origin index"),
                "{source}: {error}"
            );
            assert_eq!(rill::backend::emit(sources.program()).unwrap(), before);
        }
    }
    for source in [
        "type E = Empty | Has(i32)\nfn main() { Empty }",
        "type Marker<a> = Marker\nfn take(x: Marker<i32>) { 42 }\nfn main() { take(Marker) }",
    ] {
        let tree = Tree::new(source);
        let sources = tree.load();
        let before = rill::backend::emit(sources.program()).unwrap();
        let mapped = lower_main_with_function_origins(sources).unwrap();
        assert_eq!(rill::typed::emit_c(mapped.typed()).unwrap(), before);
    }
}
