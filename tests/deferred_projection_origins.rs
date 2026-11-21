//! Deferred SCC projections preserve ordinary loader and optional-origin contracts.
//! Fixtures are retained under TMPDIR for acceptance inspection.
use rill::{
    ast::{Expr, ParamMode},
    check, interpreter,
    modules::{
        compile::{load_program, load_program_snapshot, load_program_with_sources},
        provenance::{Lowering, SourceProgram},
        resolve::Kind,
    },
    source_map::AstNode,
    syntax::SyntaxTree,
    typed::{
        self,
        provenance::{
            lower_main_with_copy_node_origins,
            nodes::{
                CopyNodeOriginProgram, DirectRule, NodeOrigin, SourceDisposition,
                SourceRepresentation, TypedChildRole, TypedNodeRef,
            },
            ConcreteType, FunctionOriginProgram,
        },
        CallArg, ExprKind,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn fixture(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rill-deferred-projection-origins-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    for (name, source) in files {
        fs::write(dir.join(name), source).unwrap();
    }
    dir.join("main.rill")
}

fn ordered(first: &str, later: &str, reverse: bool) -> String {
    if reverse {
        format!("{later}\n{first}")
    } else {
        format!("{first}\n{later}")
    }
}

fn equal_loaders(entry: &Path) -> SourceProgram {
    let ordinary = load_program(entry).unwrap();
    let snapshot = load_program_snapshot(entry).unwrap();
    let mapped = load_program_with_sources(entry).unwrap();
    assert_eq!(snapshot.program(), &ordinary);
    assert_eq!(mapped.program(), &ordinary);
    for source in mapped.sources() {
        assert_eq!(snapshot.source(&source.module), Some(source.text.as_str()));
        let syntax = SyntaxTree::parse(&source.text);
        let ast_map = syntax.ast_source_map().unwrap();
        for origin in mapped
            .nodes()
            .iter()
            .flat_map(|node| &node.origins)
            .filter(|origin| origin.source == source.id)
        {
            let parsed = ast_map.origin(&origin.path).unwrap();
            assert_eq!(origin.span, parsed.span);
            assert_eq!(origin.provenance, parsed.provenance);
            assert_eq!(
                mapped.origin_text(origin),
                Some(&source.text[parsed.span.byte_range()])
            );
        }
    }
    mapped
}

fn equal_errors(entry: &Path, fragment: &str) {
    let ordinary = load_program(entry).unwrap_err();
    let snapshot = match load_program_snapshot(entry) {
        Ok(_) => panic!("snapshot unexpectedly accepted {entry:?}"),
        Err(error) => error,
    };
    let mapped = load_program_with_sources(entry).unwrap_err();
    assert_eq!(
        (&ordinary.code, &ordinary.message),
        (&snapshot.code, &snapshot.message)
    );
    assert_eq!(
        (&ordinary.code, &ordinary.message),
        (&mapped.code, &mapped.message)
    );
    assert!(ordinary.to_string().contains(fragment), "{ordinary}");
}

fn mapped_result(sources: SourceProgram, expected: &str) -> CopyNodeOriginProgram {
    let checked = check::analyze(sources.program()).unwrap();
    let ordinary = typed::lower_main(&checked).unwrap();
    typed::verify(&ordinary).unwrap();
    assert_eq!(typed::evaluate(&ordinary).unwrap().to_string(), expected);
    assert_eq!(
        interpreter::run(sources.program()).unwrap().to_string(),
        expected
    );
    assert_eq!(
        interpreter::run_reference(sources.program())
            .unwrap()
            .to_string(),
        expected
    );
    let c = typed::emit_c(&ordinary).unwrap();
    assert_eq!(rill::backend::emit(sources.program()).unwrap(), c);
    assert!(!c.contains("rl_value"));
    assert!(!c.contains("RL_CAP"));
    let mapped = lower_main_with_copy_node_origins(sources).unwrap();
    typed::verify(mapped.typed()).unwrap();
    assert_eq!(
        typed::evaluate(mapped.typed()).unwrap().to_string(),
        expected
    );
    assert_eq!(typed::emit_c(mapped.typed()).unwrap(), c);
    mapped
}

fn nominal<'a>(
    ty: &'a ConcreteType,
    origins: &FunctionOriginProgram,
    module: &str,
    name: &str,
) -> &'a [ConcreteType] {
    let ConcreteType::Nominal {
        definition,
        arguments,
    } = ty
    else {
        panic!("expected {module}.{name}, got {ty:?}");
    };
    let declaration = origins.definition(definition).unwrap();
    assert_eq!(declaration.symbol.kind, Kind::Type);
    assert_eq!(declaration.symbol.name, name);
    assert_eq!(
        origins.sources().source(declaration.source).unwrap().module,
        module
    );
    arguments
}

#[test]
fn imported_deferred_results_preserve_both_generic_arguments_and_all_loaders() {
    let types =
        "export type Cell<a> = {padding:bool;value:a}\nexport type Wrap<a,b> = {marker:b;value:a}";
    let first = "fn first(x,tag){let r=later(x,tag);r.value.value}";
    let later = "fn later(x,tag){if true{record t.Wrap{marker=tag;value=record t.Cell{padding=true;value=x}}}else{record t.Wrap{marker=tag;value=record t.Cell{padding=false;value=first(x,tag)}}}}";
    for reverse in [false, true] {
        let source = format!(
            "import types as t;\n{}\nfn main(){{if first(true,0){{first(42,false)}}else{{0}}}}",
            ordered(first, later, reverse)
        );
        let mapped = mapped_result(
            equal_loaders(&fixture(&[("types.rill", types), ("main.rill", &source)])),
            "42",
        );
        let origins = mapped.function_origins();
        let mut first_signatures = Vec::new();
        let mut later_signatures = Vec::new();
        for instance in origins.instances() {
            let definition = origins.definition(&instance.definition).unwrap();
            match definition.symbol.name.as_str() {
                "first" => {
                    assert_eq!(instance.parameters.len(), 2);
                    assert!(instance.parameters.iter().all(|p| p.0 == ParamMode::Value));
                    assert_eq!(instance.result, instance.parameters[0].1);
                    first_signatures.push(instance.parameters.clone());
                }
                "later" => {
                    let arguments = nominal(&instance.result, origins, "types", "Wrap");
                    assert_eq!(arguments.len(), 2);
                    let payload = nominal(&arguments[0], origins, "types", "Cell");
                    assert_eq!(payload, std::slice::from_ref(&instance.parameters[0].1));
                    assert_eq!(arguments[1], instance.parameters[1].1);
                    later_signatures.push(instance.parameters.clone());
                }
                "main" => {}
                name => panic!("unexpected instance {name}"),
            }
        }
        let expected = [
            vec![
                (ParamMode::Value, ConcreteType::Bool),
                (ParamMode::Value, ConcreteType::Int),
            ],
            vec![
                (ParamMode::Value, ConcreteType::Int),
                (ParamMode::Value, ConcreteType::Bool),
            ],
        ];
        for signatures in [first_signatures, later_signatures] {
            assert_eq!(signatures.len(), expected.len());
            for signature in &expected {
                assert!(signatures.contains(signature));
            }
        }
    }
}

const MODE_TYPES: &str =
    "export type Cell<a> = {other:i32;value:a}\nexport type Wrap<a> = {marker:bool;inner:a}";
const MODE_FIRST: &str = "fn first(x){let mut r=later(x);\n// λ: each occurrence retains its own span\nr.inner.value <- r.inner.value+1;let seen=inspect(r.inner.value);bump(inout r.inner.value);r.inner.value}";
const MODE_LATER: &str = "fn later(x:t.Wrap<t.Cell<i32>>){if true{x}else{record t.Wrap{marker=false;inner=record t.Cell{other=0;value=first(x)}}}}";

#[test]
fn deferred_read_assignment_and_loan_operands_keep_exact_source_roles() {
    for reverse in [false, true] {
        let source = format!(
            "import types as t;\nfn inspect(read x){{x}}\nfn bump(inout x){{x <- x+1}}\n{}\nfn main(){{first(record t.Wrap{{marker=true;inner=record t.Cell{{other=7;value=40}}}})}}",
            ordered(MODE_FIRST, MODE_LATER, reverse)
        );
        let sources = equal_loaders(&fixture(&[
            ("types.rill", MODE_TYPES),
            ("main.rill", &source),
        ]));
        let checked = check::analyze(sources.program()).unwrap();
        let record_names: Vec<_> = ["Wrap", "Cell"]
            .into_iter()
            .map(|name| {
                let symbol = sources
                    .symbols()
                    .iter()
                    .find(|s| s.symbol.name == name)
                    .unwrap();
                let Some(AstNode::TypeDeclaration(declaration)) = sources.node(&symbol.target)
                else {
                    panic!("record declaration");
                };
                declaration.name.clone()
            })
            .collect();
        let mut source_paths = Vec::new();
        for node in sources.nodes() {
            let Some(AstNode::Expression(expr)) = sources.node(&node.path) else {
                continue;
            };
            if matches!(expr, Expr::Field(root, _) | Expr::InoutField(root, _) | Expr::FieldAssign(root, _, _) if root == "r")
            {
                let fields = checked.field_info(expr).unwrap();
                assert_eq!(fields.len(), 2);
                for (field, name) in fields.iter().zip(&record_names) {
                    assert_eq!(&field.record, name);
                    assert_eq!((field.index, field.arity), (1, 2));
                }
                assert_eq!(node.origins.len(), 1);
                assert_eq!(node.origins[0].lowering, Lowering::Unchanged);
                source_paths.push(node.path.clone());
            }
        }
        assert_eq!(source_paths.len(), 5);
        let mapped = mapped_result(sources, "42");
        let mut counts = [0; 4];
        let mut roots = Vec::new();
        let mut represented = Vec::new();
        for node in mapped.nodes() {
            let (place, category, rule) = match mapped.typed_node(&node.id) {
                Some(TypedNodeRef::Expression(typed::ExprNode {
                    kind: ExprKind::FieldRead(place),
                    ..
                })) => (place, 0, DirectRule::Expression),
                Some(TypedNodeRef::Expression(typed::ExprNode {
                    kind: ExprKind::FieldAssign(place, _),
                    ..
                })) => (place, 1, DirectRule::Expression),
                Some(TypedNodeRef::Argument(CallArg::ReadField(place))) => {
                    (place, 2, DirectRule::Argument)
                }
                Some(TypedNodeRef::Argument(CallArg::InoutField(place))) => {
                    (place, 3, DirectRule::Argument)
                }
                _ => continue,
            };
            let NodeOrigin::SourceDerived {
                source,
                rule: actual,
            } = &node.origin
            else {
                panic!("ordinary deferred projection must retain its source occurrence");
            };
            assert_eq!(*actual, rule);
            let origin = mapped.direct_source(&node.id).unwrap();
            assert!(source_paths.contains(&origin.path));
            assert!(!represented.contains(&origin.path));
            represented.push(origin.path.clone());
            assert_eq!(place.path.len(), 2);
            assert!(place.path.iter().all(|step| step.field == 1));
            roots.push(place.root);
            counts[category] += 1;
            let SourceRepresentation::Represented(rows) = mapped
                .source_representation(&node.instance, source)
                .unwrap()
            else {
                panic!("projection source must be represented");
            };
            if category >= 2 {
                assert_eq!(rows, &[SourceDisposition::OperandOnly(node.id.clone())]);
                assert!(mapped.children(&node.id).unwrap().is_empty());
                assert_eq!(node.role, TypedChildRole::CallArgument(0));
            } else {
                assert!(rows.contains(&SourceDisposition::Output(node.id.clone())));
            }
            if category == 1 {
                let children = mapped.children(&node.id).unwrap();
                assert_eq!(children.len(), 1);
                assert_eq!(
                    mapped.node(&children[0]).unwrap().role,
                    TypedChildRole::AssignmentValue
                );
            }
        }
        assert_eq!(counts, [2, 1, 1, 1]);
        assert!(roots.iter().all(|root| *root == roots[0]));
        assert_eq!(represented.len(), source_paths.len());
    }
}

#[test]
fn a_shadowed_import_alias_and_local_keep_distinct_projection_roots() {
    for reverse in [false, true] {
        let first = "fn first(x){let t=later(x);let old=t.value;let t=record t.Cell{other=0;value=2};old+t.value}";
        let later =
            "fn later(x:t.Cell<i32>){if true{x}else{record t.Cell{other=0;value=first(x)}}}";
        let source = format!(
            "import types as t;\n{}\nfn main(){{first(record t.Cell{{other=0;value=40}})}}",
            ordered(first, later, reverse)
        );
        let mapped = mapped_result(
            equal_loaders(&fixture(&[
                ("types.rill", MODE_TYPES),
                ("main.rill", &source),
            ])),
            "42",
        );
        let mut roots = Vec::new();
        let mut spans = Vec::new();
        for node in mapped.nodes() {
            if let Some(TypedNodeRef::Expression(typed::ExprNode {
                kind: ExprKind::FieldRead(place),
                ..
            })) = mapped.typed_node(&node.id)
            {
                let source = mapped.direct_source(&node.id).unwrap();
                assert_eq!(
                    mapped.sources().origin_text(&source.origins[0]),
                    Some("t.value")
                );
                assert_eq!(source.origins[0].lowering, Lowering::Unchanged);
                assert!(
                    matches!(mapped.sources().node(&source.path), Some(AstNode::Expression(Expr::Field(root, _))) if root == "t")
                );
                roots.push(place.root);
                spans.push(source.origins[0].span);
            }
        }
        assert_eq!(roots.len(), 2);
        assert_ne!(roots[0], roots[1]);
        assert_ne!(spans[0], spans[1]);
    }
}

#[test]
fn deferred_inferred_exports_cannot_leak_private_nominal_results() {
    for reverse in [false, true] {
        for payload in [
            "record Hidden{value=42}",
            "record Wrap{value=record Hidden{value=42}}",
        ] {
            let first = "export fn first(){let r=later();r.value}";
            let later = format!("fn later(){{if true{{record Box{{value={payload}}}}}else{{record Box{{value=first()}}}}}}");
            let api = format!(
                "type Hidden={{value:i32}}\nexport type Wrap<a> = {{value:a}}\ntype Box<a> = {{value:a}}\n{}",
                ordered(first, &later, reverse)
            );
            equal_errors(
                &fixture(&[
                    ("api.rill", &api),
                    ("main.rill", "import api;fn main(){42}"),
                ]),
                "E-MODULE-PRIVATE-TYPE",
            );
        }
    }
}

#[test]
fn deferred_constraints_never_merge_same_labelled_module_records() {
    for reverse in [false, true] {
        let first = "fn first(x){let r=later(x);r.value}";
        let later = "fn later(x:b.P){if true{x}else{record b.P{value=first(x)}}}";
        let source = format!(
            "import a;import b;{}\nfn main(){{first(record a.P{{value=42}})}}",
            ordered(first, later, reverse)
        );
        equal_errors(
            &fixture(&[
                ("a.rill", "export type P={value:i32}"),
                ("b.rill", "export type P={value:i32}"),
                ("main.rill", &source),
            ]),
            "E-TYPE",
        );
    }
    let source = "import a;import b;fn project(p){p.value}fn main(){project(record a.P{value=42})}";
    equal_errors(
        &fixture(&[
            ("a.rill", "export type P={value:i32}"),
            ("b.rill", "export type P={value:i32}"),
            ("main.rill", source),
        ]),
        "E-RECORD",
    );
}

#[test]
fn equal_named_deferred_groups_keep_separate_module_definition_identities() {
    for reverse in [false, true] {
        let first = "export fn first(x){let r=later(x);r.value}";
        let later = "fn later(x){if true{record P{value=x}}else{record P{value=first(x)}}}";
        let module = format!(
            "export type P<a> = {{value:a}}\n{}",
            ordered(first, later, reverse)
        );
        let sources = equal_loaders(&fixture(&[
            ("a.rill", &module),
            ("b.rill", &module),
            (
                "main.rill",
                "import a;import b;fn main(){a.first(20)+b.first(22)}",
            ),
        ]));
        let mapped = mapped_result(sources, "42");
        let origins = mapped.function_origins();
        let mut definitions = Vec::new();
        let mut modules = Vec::new();
        for instance in origins.instances() {
            let function = origins.definition(&instance.definition).unwrap();
            if function.symbol.name != "later" {
                continue;
            }
            let module = &origins.sources().source(function.source).unwrap().module;
            assert_eq!(
                nominal(&instance.result, origins, module, "P"),
                &[ConcreteType::Int]
            );
            let ConcreteType::Nominal { definition, .. } = &instance.result else {
                unreachable!()
            };
            definitions.push(definition.clone());
            modules.push(module.as_str());
        }
        assert_eq!(definitions.len(), 2);
        assert_ne!(definitions[0], definitions[1]);
        assert!(modules.contains(&"a"));
        assert!(modules.contains(&"b"));
        let projection_sources: Vec<_> = mapped
            .nodes()
            .iter()
            .filter_map(|node| {
                let Some(TypedNodeRef::Expression(typed::ExprNode {
                    kind: ExprKind::FieldRead(_),
                    ..
                })) = mapped.typed_node(&node.id)
                else {
                    return None;
                };
                let source = mapped.direct_source(&node.id).unwrap();
                assert_eq!(
                    mapped.sources().origin_text(&source.origins[0]),
                    Some("r.value")
                );
                Some(source.origins[0].source)
            })
            .collect();
        assert_eq!(projection_sources.len(), 2);
        assert_ne!(projection_sources[0], projection_sources[1]);
    }
}

#[test]
fn optional_origins_keep_native_and_opaque_boundaries_after_deferral() {
    for reverse in [false, true] {
        let first = "fn first(x){let r=later(x);r.value}";
        let later = "fn later(x:P){if true{x}else{record P{value=first(x)}}}";
        let native = format!(
            "extern fn unused(x:i32)->i32;type P={{value:i32}}\n{}\nfn main(){{first(record P{{value=42}})}}",
            ordered(first, later, reverse)
        );
        let sources = equal_loaders(&fixture(&[("main.rill", &native)]));
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = typed::lower_main(&checked).unwrap();
        assert_eq!(typed::evaluate(&ordinary).unwrap().to_string(), "42");
        assert!(lower_main_with_copy_node_origins(sources)
            .unwrap_err()
            .contains("does not map native"));

        let first = "fn first(x){let r=later(x);r.marker}";
        let later = "fn later(x){if true{record Box{marker=Off;value=x}}else{record Box{marker=first(x);value=x}}}";
        let opaque = format!(
            "type Flag<a> = Off|On\ntype Box<a,b> = {{marker:a;value:b}}\n{}\nfn main(){{let mut flag=first(42);42}}",
            ordered(first, later, reverse)
        );
        let sources = equal_loaders(&fixture(&[("main.rill", &opaque)]));
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = typed::lower_main(&checked).unwrap();
        typed::verify(&ordinary).unwrap();
        assert_eq!(typed::evaluate(&ordinary).unwrap().to_string(), "42");
        assert!(lower_main_with_copy_node_origins(sources)
            .unwrap_err()
            .contains("E-TYPED-UNSUPPORTED"));
    }
}
