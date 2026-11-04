use rill::{
    backend, check, formatter,
    modules::compile::{load_program, load_program_with_sources},
    source_map::{AstNode, AstStep},
    syntax::{AstDeclaration, SyntaxTree},
    typed::provenance::{
        lower_main_with_copy_node_origins,
        nodes::{
            GenerationReason, NodeOrigin, RelationRole, RelationTarget, SourceDisposition,
            SourceRepresentation,
        },
    },
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn fixture(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rill-result-annotations-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    for (name, source) in files {
        fs::write(dir.join(name), source).unwrap();
    }
    // Retain newly introduced annotation fixtures for independent inspection.
    dir.join("main.rill")
}

#[test]
fn result_annotation_has_its_own_exact_cst_and_ast_role() {
    let source = "// λ\nfn value(x: O<i32>) -> O<// nested\ni32> { x }\nfn main(){42}";
    let tree = SyntaxTree::parse(source);
    assert_eq!(tree.reconstruct(), source);
    let module = tree.parsed_module().unwrap();
    let map = tree.ast_source_map().unwrap();
    let root = rill::source_map::AstPath::root(AstDeclaration::Function(0));
    let result = root.child(AstStep::FunctionResult);
    assert!(matches!(result.resolve(module), Some(AstNode::TypeExpression(ty)) if ty.name == "O"));
    assert!(map.origin(&result).is_some());
    assert!(map
        .origin(&result.child(AstStep::TypeArgument(0)))
        .is_some());
    assert_eq!(
        AstNode::Function(&module.program.functions[0]).child_steps(),
        vec![
            AstStep::Parameter(0),
            AstStep::FunctionResult,
            AstStep::FunctionBody
        ]
    );
    assert!(root.child(AstStep::ForeignResult).resolve(module).is_none());
    let output = formatter::format(source).unwrap();
    assert_eq!(formatter::format(&output).unwrap(), output);
    assert_eq!(rill::parser::parse_module(&output).unwrap(), *module);
}

#[test]
fn exported_nested_results_rewrite_and_map_every_annotation_node() {
    let entry = fixture(&[
        ("types.rill", "export type O<a> = None | Some(a)\nexport type P = { x:i32 }\nexport fn absent() -> O<P> {None}"),
        ("main.rill", "import types as t;\nfn identity(x:t.O<t.P>) -> t.O<t.P> {x}\nfn main() -> i32 { match identity(t.absent()) { t.None => 42 | t.Some(p) => p.x } }"),
    ]);
    let ordinary = load_program(&entry).unwrap();
    let sources = load_program_with_sources(&entry).unwrap();
    assert_eq!(*sources.program(), ordinary);
    let expected_c = backend::emit(&ordinary).unwrap();
    let mapped = lower_main_with_copy_node_origins(sources).unwrap();
    assert_eq!(
        rill::typed::evaluate(mapped.typed()).unwrap().to_string(),
        "42"
    );
    assert_eq!(
        backend::emit(mapped.sources().program()).unwrap(),
        expected_c
    );
    let mut annotated = 0;
    for node in mapped.nodes() {
        if let NodeOrigin::Generated {
            reason: GenerationReason::AnnotatedResultType,
            related,
        } = &node.origin
        {
            annotated += 1;
            let relations = mapped.relations(related).unwrap();
            assert_eq!(
                relations.iter().map(|r| r.role).collect::<Vec<_>>(),
                vec![
                    RelationRole::Definition,
                    RelationRole::Body,
                    RelationRole::Annotation
                ]
            );
            let RelationTarget::Source(annotation) = &relations[2].target else {
                panic!("source annotation");
            };
            let record = mapped.function_origins().source_node(annotation).unwrap();
            assert_eq!(record.path.steps, vec![AstStep::FunctionResult]);
            assert!(
                matches!(mapped.source_representation(&node.instance, annotation).unwrap(), SourceRepresentation::Represented(rows) if rows == [SourceDisposition::SignatureOnly])
            );
        }
    }
    assert_eq!(annotated, 3);
    check::check(mapped.sources().program()).unwrap();
}

#[test]
fn result_annotations_preserve_private_type_errors_and_loader_equality() {
    for api in ["type Hidden = Hidden\nexport fn hidden() -> Hidden {Hidden}", "type Hidden = Hidden\nexport type O<a> = None | Some(a)\nexport fn hidden() -> O<Hidden> {None}"] {
        let entry = fixture(&[("api.rill", api), ("main.rill", "import api; fn main(){42}")]);
        let ordinary = load_program(&entry).unwrap_err();
        let mapped = load_program_with_sources(&entry).unwrap_err();
        assert_eq!(ordinary.code, mapped.code);
        assert_eq!(ordinary.message, mapped.message);
        assert!(ordinary.to_string().contains("E-MODULE-PRIVATE-TYPE"));
    }
}

#[test]
fn unused_annotations_keep_definitions_without_creating_instances() {
    use rill::typed::provenance::FunctionDisposition;
    let entry = fixture(&[(
        "main.rill",
        "type O<a> = None|Some(a) fn unused() -> O<i32> {None} fn main() -> i32 {42}",
    )]);
    let mapped =
        lower_main_with_copy_node_origins(load_program_with_sources(&entry).unwrap()).unwrap();
    let origins = mapped.function_origins();
    let unused = origins
        .function_definitions()
        .find(|id| origins.definition(id).unwrap().symbol.name == "unused")
        .unwrap();
    assert_eq!(
        origins.function_disposition(&unused),
        Some(FunctionDisposition::NotInstantiated)
    );
    assert_eq!(origins.instances().len(), 1);
}

#[test]
fn annotations_preserve_copy_origin_native_owner_and_contextual_boundaries() {
    for source in [
        "fn make()->SystemBoxI32{let b=system_box_i32_abort(42);move b} fn main()->i32{let b=make();box_read(b)}",
        "type O<a> = None|Some(a) fn make()->O<SystemBoxI32>{None} fn main()->i32{let b=make();match move b{None=>42|Some(b)=>box_read(b)}}",
        "fn make()->u8{u8(42)} fn main()->bool{make()==u8(42)}",
        "extern fn unused()->i32; fn main()->i32{42}",
        "type O<a> = None|Some(a) fn absent(){None} fn main()->i32{let ignored=absent();42}",
    ] {
        let entry = fixture(&[("main.rill", source)]);
        let ordinary = load_program(&entry).unwrap();
        backend::emit(&ordinary).unwrap();
        let error = lower_main_with_copy_node_origins(load_program_with_sources(&entry).unwrap()).unwrap_err();
        assert!(error.starts_with("E-TYPED-UNSUPPORTED"), "{source}: {error}");
    }
}

#[test]
fn imported_result_identity_uses_type_namespace_and_never_shape_equivalence() {
    let a = "export type T = A export fn T() -> T {A}";
    let b = "export type T = B export fn make() -> T {B}";
    let entry = fixture(&[("a.rill", a), ("b.rill", b), ("main.rill", "import a; import b; fn left()->a.T{a.T()} fn right()->b.T{b.make()} fn main()->i32{let l=match left(){a.A=>20};let r=match right(){b.B=>22};l+r}")]);
    let sources = load_program_with_sources(&entry).unwrap();
    assert_eq!(*sources.program(), load_program(&entry).unwrap());
    let mapped = lower_main_with_copy_node_origins(sources).unwrap();
    assert_eq!(
        rill::typed::evaluate(mapped.typed()).unwrap().to_string(),
        "42"
    );
    let entry = fixture(&[
        ("a.rill", a),
        ("b.rill", b),
        (
            "main.rill",
            "import a; import b; fn wrong()->b.T{a.T()} fn main(){42}",
        ),
    ]);
    let error = load_program(&entry).unwrap_err();
    assert!(error.message.starts_with("E-TYPE"), "{error}");
}

#[test]
fn unused_private_result_annotations_reject_even_through_phantoms() {
    for api in [
        "type Hidden=Hidden export type Public<a> = Empty export fn hidden()->Public<Hidden>{Empty}",
        "type Hidden=Hidden export type Public<a> = Empty export type Outer<a> = Wrap(a) export fn hidden()->Outer<Public<Hidden>>{Wrap(Empty)}",
    ] {
        let entry = fixture(&[("api.rill", api), ("main.rill", "import api; fn main()->i32{42}")]);
        let error = load_program(&entry).unwrap_err();
        let mapped = load_program_with_sources(&entry).unwrap_err();
        assert_eq!((error.code,error.message.clone()), (mapped.code,mapped.message));
        assert!(error.to_string().contains("E-MODULE-PRIVATE-TYPE"), "{error}");
    }
    let entry = fixture(&[
        ("api.rill", "type Hidden=Hidden export fn value(){42}"),
        (
            "main.rill",
            "import api; fn unused()->api.Hidden{unused()} fn main(){42}",
        ),
    ]);
    let error = load_program(&entry).unwrap_err();
    assert!(error.to_string().contains("E-MODULE-PRIVATE"), "{error}");
}

#[test]
fn raw_pointee_annotation_names_stay_outside_rill_module_resolution() {
    let entry = fixture(&[
        (
            "api.rill",
            "export type AnnHandle=Handle export fn raw()->ptr<AnnHandle>{c_null()}",
        ),
        (
            "main.rill",
            "import api; fn raw()->ptr<AnnHandle>{api.raw()} fn main()->bool{is_null(raw())}",
        ),
    ]);
    let program = load_program(&entry).unwrap();
    assert_eq!(
        *load_program_with_sources(&entry).unwrap().program(),
        program
    );
    for function in &program.functions {
        if let Some(ty) = &function.result_annotation {
            if ty.name == "ptr" {
                assert_eq!(ty.args[0].name, "AnnHandle");
            }
        }
    }
    for source in [
        "fn wrong(p:ptr<AnnHandle>)->const_ptr<AnnHandle>{p} fn main(){42}",
        "fn wrong(p:ptr<AnnHandle>)->ptr<OtherHandle>{p} fn main(){42}",
    ] {
        let entry = fixture(&[("main.rill", source)]);
        assert!(load_program(&entry)
            .unwrap_err()
            .message
            .starts_with("E-TYPE"));
    }
}
