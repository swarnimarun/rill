//! Closed parameter preconstraints only, not general body-order independence.
//! Fixtures are retained under TMPDIR for independent acceptance inspection.
use rill::{
    ast::ParamMode,
    check, interpreter,
    modules::{
        compile::{load_program, load_program_snapshot, load_program_with_sources},
        provenance::{Lowering, SourceProgram},
        resolve::Kind,
    },
    source_map::{AstNode, AstStep},
    syntax::SyntaxTree,
    typed::{
        self,
        provenance::{
            lower_main_with_copy_node_origins,
            nodes::{
                DirectRule, GenerationReason, NodeOrigin, RelationRole, RelationTarget,
                SourceDisposition, SourceRepresentation,
            },
            ConcreteType,
        },
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
        "rill-recursive-signature-origins-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    for (name, source) in files {
        fs::write(dir.join(name), source).unwrap();
    }
    dir.join("main.rill")
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
    // ProgramSnapshot intentionally has no Debug implementation.
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

const TYPES: &str = "export type P = { x:i32 }\nexport type Wrap<a> = { value:a }";
const FIRST: &str = "fn first(p,n) -> i32 { let ignored = later(p,0); p.value.x }";
const LATER: &str = "fn later(t:t.Wrap<// λ nested imported type\nt.P>,n:i32) -> i32 { if n == 0 {42} else {first(t,n-1)} }";
fn recursive_fixture(reverse: bool) -> PathBuf {
    let functions = if reverse {
        format!("{LATER}\n{FIRST}")
    } else {
        format!("{FIRST}\n{LATER}")
    };
    let source = format!(
        "import types as t;\n{functions}\nfn main() -> i32 {{ first(record t.Wrap {{ value = record t.P {{x=42}} }},2) }}"
    );
    fixture(&[("types.rill", TYPES), ("main.rill", &source)])
}

#[test]
fn later_parameter_constraint_precedes_first_body_in_all_loader_forms() {
    // These two specific permutations require the closed peer parameter. This
    // is not a claim that historical unannotated result projections commute.
    for reverse in [false, true] {
        let sources = equal_loaders(&recursive_fixture(reverse));
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = typed::lower_main(&checked).unwrap();
        let c = typed::emit_c(&ordinary).unwrap();
        assert_eq!(
            interpreter::run_reference(sources.program())
                .unwrap()
                .to_string(),
            "42"
        );
        assert_eq!(typed::evaluate(&ordinary).unwrap().to_string(), "42");
        let mapped = lower_main_with_copy_node_origins(sources).unwrap();
        assert_eq!(typed::evaluate(mapped.typed()).unwrap().to_string(), "42");
        assert_eq!(typed::emit_c(mapped.typed()).unwrap(), c);
        let origins = mapped.function_origins();
        assert_eq!(origins.instances().len(), 3);
        for instance in origins.instances() {
            let definition = origins.definition(&instance.definition).unwrap();
            assert_eq!(instance.result, ConcreteType::Int);
            if definition.symbol.name == "main" {
                assert!(instance.parameters.is_empty());
                continue;
            }
            assert!(matches!(definition.symbol.name.as_str(), "first" | "later"));
            assert_eq!(instance.parameters.len(), 2);
            assert_eq!(
                instance.parameters[1],
                (ParamMode::Value, ConcreteType::Int)
            );
            assert_eq!(instance.parameters[0].0, ParamMode::Value);
            let ConcreteType::Nominal {
                definition,
                arguments,
            } = &instance.parameters[0].1
            else {
                panic!("closed Wrap parameter");
            };
            let wrap = origins.definition(definition).unwrap();
            assert_eq!(wrap.symbol.name, "Wrap");
            assert_eq!(
                origins.sources().source(wrap.source).unwrap().module,
                "types"
            );
            let [ConcreteType::Nominal {
                definition,
                arguments,
            }] = arguments.as_slice()
            else {
                panic!("closed P argument");
            };
            let point = origins.definition(definition).unwrap();
            assert_eq!(point.symbol.name, "P");
            assert_eq!(
                origins.sources().source(point.source).unwrap().module,
                "types"
            );
            assert!(arguments.is_empty());
        }
    }
}

#[test]
fn nested_parameter_spans_and_typed_roles_are_not_function_results() {
    let mapped =
        lower_main_with_copy_node_origins(equal_loaders(&recursive_fixture(false))).unwrap();
    let origins = mapped.function_origins();
    let later = origins
        .instances()
        .iter()
        .find(|instance| {
            origins
                .definition(&instance.definition)
                .unwrap()
                .symbol
                .name
                == "later"
        })
        .unwrap();
    let root = &origins.definition(&later.definition).unwrap().target;
    let annotation = root.child(AstStep::Parameter(0)).child(AstStep::Annotation);
    let argument = annotation.child(AstStep::TypeArgument(0));
    let result = root.child(AstStep::FunctionResult);
    for (path, text, name) in [
        (
            &annotation,
            "t.Wrap<// λ nested imported type\nt.P>",
            "Wrap",
        ),
        (&argument, "t.P", "P"),
        (&result, "i32", "i32"),
    ] {
        let node = mapped.sources().origin(path).unwrap();
        assert_eq!(node.origins.len(), 1);
        assert_eq!(mapped.sources().origin_text(&node.origins[0]), Some(text));
        if name != "i32" {
            let Lowering::ResolvedName(id) = node.origins[0].lowering else {
                panic!("imported annotation must carry its type identity");
            };
            let symbol = mapped.sources().symbol(id).unwrap();
            assert_eq!(symbol.symbol.kind, Kind::Type);
            assert_eq!(symbol.symbol.name, name);
        }
        let (source, _) = mapped
            .source_nodes()
            .find(|(_, candidate)| candidate.path == *path)
            .unwrap();
        assert!(
            matches!(mapped.source_representation(&later.id, &source).unwrap(),
            SourceRepresentation::Represented(rows) if rows == [SourceDisposition::SignatureOnly])
        );
    }
    assert!(mapped
        .sources()
        .node(&root.child(AstStep::Annotation))
        .is_none());
    assert!(mapped
        .sources()
        .node(&annotation.child(AstStep::FunctionResult))
        .is_none());
    let mut parameters = 0;
    let mut results = 0;
    for node in mapped
        .nodes()
        .iter()
        .filter(|node| node.instance == later.id)
    {
        match &node.origin {
            NodeOrigin::SourceDerived {
                source,
                rule: DirectRule::Parameter,
            } => {
                let path = &origins.source_node(source).unwrap().path;
                assert!(matches!(path.steps.as_slice(), [AstStep::Parameter(0 | 1)]));
                parameters += 1;
            }
            NodeOrigin::Generated {
                reason: GenerationReason::AnnotatedResultType,
                related,
            } => {
                let relations = mapped.relations(related).unwrap();
                assert_eq!(
                    relations.iter().map(|r| r.role).collect::<Vec<_>>(),
                    vec![
                        RelationRole::Definition,
                        RelationRole::Body,
                        RelationRole::Annotation
                    ]
                );
                let RelationTarget::Source(source) = &relations[2].target else {
                    panic!("written result annotation");
                };
                assert_eq!(origins.source_node(source).unwrap().path, result);
                results += 1;
            }
            _ => {}
        }
    }
    assert_eq!((parameters, results), (2, 1));
}

#[test]
fn unused_exported_legacy_and_phantom_parameter_annotations_keep_privacy() {
    for api in [
        "type Hidden=Hidden export fn unused(x:Hidden){42}",
        "type Hidden=Hidden export let unused (x:Hidden) = 42",
        "type Hidden=Hidden export type Phantom<a> = Empty export fn unused(x:Phantom<Hidden>){42}",
        "type Hidden=Hidden export type Phantom<a> = Empty export type Wrap<a> = Wrap(a) export let unused (x:Wrap<Phantom<Hidden>>) = 42",
    ] {
        let entry = fixture(&[("api.rill", api), ("main.rill", "import api; fn main(){42}")]);
        equal_errors(&entry, "E-MODULE-PRIVATE-TYPE");
    }
}

#[test]
fn unused_imported_private_parameter_types_reject_including_phantoms() {
    for annotation in ["api.Hidden", "api.Phantom<api.Hidden>"] {
        let main = format!("import api; fn unused(x:{annotation}){{42}} fn main(){{42}}");
        let entry = fixture(&[
            (
                "api.rill",
                "type Hidden=Hidden export type Phantom<a> = Empty",
            ),
            ("main.rill", &main),
        ]);
        equal_errors(&entry, "E-MODULE-PRIVATE");
    }
}

#[test]
fn equal_record_shapes_and_phantom_layouts_do_not_unify_nominal_parameters() {
    for (declarations, main) in [
        ("export type P={x:i32}", "import a; import b; fn first(p)->i32{later(p)} fn later(p:b.P)->i32{if true{p.x}else{first(p)}} fn main(){first(record a.P{x=42})}"),
        ("export type P<a> = Empty", "import a; import b; fn first(p)->i32{later(p)} fn later(p:b.P<i32>)->i32{if true{42}else{first(p)}} fn main(){first(a.Empty)}"),
    ] {
        let entry = fixture(&[("a.rill", declarations), ("b.rill", declarations), ("main.rill", main)]);
        equal_errors(&entry, "E-TYPE");
    }
}

#[test]
fn parameter_shadowing_import_alias_keeps_type_and_value_namespaces_distinct() {
    let entry = fixture(&[
        ("types.rill", TYPES),
        ("main.rill", "import types as t; fn first(p)->i32{let ignored=later(p);p.x} fn later(t:t.P)->i32{if true{t.x}else{first(t)}} fn main(){first(record t.P{x=42})}"),
    ]);
    let sources = equal_loaders(&entry);
    let field = sources
        .nodes()
        .iter()
        .find(|node| sources.origin_text(&node.origins[0]) == Some("t.x"))
        .unwrap();
    assert_eq!(field.origins[0].lowering, Lowering::Unchanged);
    assert!(matches!(
        sources.node(&field.path),
        Some(AstNode::Expression(rill::ast::Expr::Field(..)))
    ));
    let mapped = lower_main_with_copy_node_origins(sources).unwrap();
    assert_eq!(typed::evaluate(mapped.typed()).unwrap().to_string(), "42");
}

#[test]
fn nested_raw_parameter_subtrees_keep_exact_c_names_and_constness() {
    for pointer in ["ptr", "const_ptr"] {
        let api = format!("type Handle=Handle export type Phantom<a> = Empty export fn unused(x:Phantom<{pointer}<Handle>>){{42}}");
        let entry = fixture(&[
            ("api.rill", &api),
            ("main.rill", "import api; fn main(){42}"),
        ]);
        let sources = equal_loaders(&entry);
        let function = sources
            .symbols()
            .iter()
            .find(|s| s.symbol.name == "unused")
            .unwrap();
        let raw_path = function
            .target
            .child(AstStep::Parameter(0))
            .child(AstStep::Annotation)
            .child(AstStep::TypeArgument(0));
        let Some(AstNode::TypeExpression(raw)) = sources.node(&raw_path) else {
            panic!("nested raw annotation");
        };
        assert_eq!(raw.name, pointer);
        assert_eq!(raw.args.len(), 1);
        assert_eq!(raw.args[0].name, "Handle");
        assert!(raw.args[0].args.is_empty());
        for (path, text) in [
            (raw_path.clone(), format!("{pointer}<Handle>")),
            (
                raw_path.child(AstStep::TypeArgument(0)),
                "Handle".to_owned(),
            ),
        ] {
            let origin = &sources.origin(&path).unwrap().origins[0];
            assert_eq!(origin.lowering, Lowering::Unchanged);
            assert_eq!(sources.origin_text(origin), Some(text.as_str()));
        }
    }
}

#[test]
fn recursive_raw_parameters_never_erase_pointee_or_const_mismatches() {
    for (left, right) in [
        ("ptr<Handle>", "const_ptr<Handle>"),
        ("ptr<Handle>", "ptr<OtherHandle>"),
        ("const_ptr<Handle>", "const_ptr<OtherHandle>"),
    ] {
        let source = format!("fn first(p:{left})->i32{{later(p)}} fn later(p:{right})->i32{{if true{{42}}else{{first(p)}}}} fn main(){{42}}");
        equal_errors(&fixture(&[("main.rill", &source)]), "E-TYPE");
    }
}
