//! Complete Copy-body occurrences, not layout/owner/native/C-byte mapping.
use rill::{
    check, interpreter,
    modules::{compile::load_program_with_sources, provenance::SourceProgram},
    source_map::{AstStep, OriginKind},
    typed,
    typed::provenance::nodes::{
        lower_main_with_copy_node_origins, CopyNodeOriginProgram, Coverage, DirectRule,
        GenerationReason, NodeOrigin, OriginQueryError, RelationTarget, ShapeKind,
        SourceDisposition, SourceRepresentation, TypedChildRole, TypedNodeRef,
    },
    typed::provenance::{lower_main_with_function_origins, FunctionDisposition},
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-copy-body-origins-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let this = Self(path);
        this.write("main.rill", source);
        this
    }
    fn write(&self, name: &str, text: &str) {
        fs::write(self.0.join(name), text).unwrap();
    }
    fn sources(&self) -> SourceProgram {
        load_program_with_sources(&self.0.join("main.rill")).unwrap()
    }
    fn mapped(&self) -> CopyNodeOriginProgram {
        lower_main_with_copy_node_origins(self.sources()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn checked(source: &str) -> CopyNodeOriginProgram {
    let fixture = Fixture::new(source);
    let sources = fixture.sources();
    let checked = check::analyze(sources.program()).unwrap();
    let ordinary = typed::lower_main(&checked).unwrap();
    let expected = typed::evaluate(&ordinary);
    let c = typed::emit_c(&ordinary).unwrap();
    assert_eq!(interpreter::run_reference(sources.program()), expected);
    assert_eq!(rill::backend::emit(sources.program()).unwrap(), c);
    let mapped =
        lower_main_with_copy_node_origins(sources).unwrap_or_else(|e| panic!("{source}: {e}"));
    assert_eq!(mapped.coverage(), Coverage::CopyFunctionBodies);
    assert_eq!(typed::evaluate(mapped.typed()), expected);
    assert_eq!(typed::emit_c(mapped.typed()).unwrap(), c);
    for node in mapped.nodes() {
        assert_eq!(mapped.typed_node(&node.id).unwrap().kind(), node.kind);
        for child in mapped.children(&node.id).unwrap() {
            assert_eq!(mapped.node(child).unwrap().parent.as_ref(), Some(&node.id));
        }
        match &node.origin {
            NodeOrigin::SourceDerived { source, .. } => {
                assert!(!mapped
                    .function_origins()
                    .source_node(source)
                    .unwrap()
                    .origins
                    .is_empty());
                assert!(
                    matches!(mapped.source_representation(&node.instance,source).unwrap(),SourceRepresentation::Represented(rows) if !rows.is_empty())
                );
            }
            NodeOrigin::Generated { related, .. } => {
                assert!(!mapped.relations(related).unwrap().is_empty())
            }
        }
    }
    mapped
}
fn normalized(mapped: &CopyNodeOriginProgram) -> Vec<String> {
    mapped
        .nodes()
        .iter()
        .map(|node| {
            let instance = mapped
                .function_origins()
                .instances()
                .iter()
                .position(|i| i.id == node.instance)
                .unwrap();
            let parent = node
                .parent
                .as_ref()
                .map(|p| mapped.nodes().iter().position(|n| &n.id == p).unwrap());
            let origin = match &node.origin {
                NodeOrigin::SourceDerived { source, rule } => format!(
                    "{rule:?} {:?}",
                    mapped.function_origins().source_node(source).unwrap().path
                ),
                NodeOrigin::Generated { reason, related } => {
                    format!("{reason:?} {:?}", mapped.relations(related).unwrap())
                }
            };
            format!(
                "{instance} {parent:?} {:?} {:?} {origin}",
                node.role, node.kind
            )
        })
        .collect()
}

#[test]
fn snapshot_branding_determinism_move_clone_and_retained_sources() {
    let fixture=Fixture::new("type P = { a:i32; b:i32 }\nfn main() { let p = record P { b=1; a=1 }; match p.a { 1 => p.b | _ => 0 } }");
    let first = fixture.mapped();
    let second = fixture.mapped();
    assert_eq!(normalized(&first), normalized(&second));
    for row in first.nodes() {
        assert!(second.node(&row.id).is_none());
        assert!(second.typed_node(&row.id).is_none());
        assert!(second.children(&row.id).is_none());
    }
    let clone = first.clone();
    let id = first.nodes()[0].id.clone();
    drop(first);
    assert!(clone.node(&id).is_some());
    fixture.write("main.rill", "fn main() { false }");
    fs::remove_file(fixture.0.join("main.rill")).unwrap();
    assert_eq!(typed::evaluate(clone.typed()).unwrap().to_string(), "1");
    assert!(clone
        .sources()
        .sources()
        .iter()
        .any(|s| s.text.contains("record P")));
    assert_eq!(clone.ancestors(&id, 0).unwrap().len(), 0);
    assert_eq!(clone.ancestors(&id, 1).unwrap().len(), 1);
}

#[test]
fn equal_literals_shadowing_and_parser_generated_origins_remain_distinct() {
    let mapped=checked("fn main() {\r\n let x = 1; let n = { let x = 1; x + 1 }; -x + n + byte_len(\"é\");\r\n}\r\n");
    let literals: Vec<_> = mapped
        .nodes()
        .iter()
        .filter(|n| {
            matches!(
                mapped.typed_node(&n.id),
                Some(TypedNodeRef::Expression(typed::ExprNode {
                    kind: typed::ExprKind::Int(1),
                    ..
                }))
            )
        })
        .collect();
    assert!(literals.len() >= 3);
    let paths: std::collections::HashSet<_> = literals
        .iter()
        .map(|n| mapped.direct_source(&n.id).unwrap().path.clone())
        .collect();
    assert_eq!(paths.len(), literals.len());
    let kinds: Vec<_> = mapped
        .nodes()
        .iter()
        .filter_map(|n| mapped.direct_source(&n.id))
        .flat_map(|s| s.origins.iter().map(|o| o.provenance))
        .collect();
    assert!(kinds.contains(&OriginKind::UnaryNegation));
    assert!(mapped
        .nodes()
        .iter()
        .filter_map(|n| mapped.direct_source(&n.id))
        .any(|s| s
            .origins
            .iter()
            .any(|o| o.span.start.byte == o.span.end.byte)));
}

#[test]
fn complete_instances_keep_return_only_types_and_unused_definitions() {
    let mapped=checked("type P<a> = Marker\nfn make(x) { Marker }\nfn ints(x:P<i32>) { 40 }\nfn bools(x:P<bool>) { 2 }\nfn unused(x) { x }\nfn main() { ints(make(0)) + bools(make(0)) }");
    let f = mapped.function_origins();
    let makes: Vec<_> = f
        .instances()
        .iter()
        .filter(|i| f.definition(&i.definition).unwrap().symbol.name == "make")
        .collect();
    assert_eq!(makes.len(), 2);
    assert_ne!(makes[0].result, makes[1].result);
    let bodies: Vec<_> = mapped
        .nodes()
        .iter()
        .filter(|n| {
            n.role == TypedChildRole::FunctionBody && makes.iter().any(|i| i.id == n.instance)
        })
        .collect();
    assert_eq!(bodies.len(), 2);
    assert_ne!(bodies[0].id, bodies[1].id);
    let (NodeOrigin::SourceDerived { source: a, .. }, NodeOrigin::SourceDerived { source: b, .. }) =
        (&bodies[0].origin, &bodies[1].origin)
    else {
        panic!("direct bodies")
    };
    assert_eq!(a, b);
    let unused = f
        .function_definitions()
        .find(|id| f.definition(id).unwrap().symbol.name == "unused")
        .unwrap();
    assert_eq!(
        f.function_disposition(&unused),
        Some(FunctionDisposition::NotInstantiated)
    );
    assert_eq!(
        mapped.source_representation(&f.entry(), a),
        Err(OriginQueryError::WrongFunction)
    );
}

#[test]
fn recursive_requests_and_imported_equal_names_have_distinct_bodies() {
    checked("fn f(x,n) { if n==0 { x } else { g(x,n-1) } }\nfn g(x,n) { f(x,n) }\nfn main() { if f(true,2) { f(42,2) } else { 0 } }");
    let fixture = Fixture::new("import a; import b; fn main() { a.answer(20) + b.answer(20) }");
    fixture.write(
        "a.rill",
        "fn same(x) { x+1 }\nexport fn answer(x) { same(x) }",
    );
    fixture.write(
        "b.rill",
        "fn same(x) { x+1 }\nexport fn answer(x) { same(x) }",
    );
    let mapped = fixture.mapped();
    let f = mapped.function_origins();
    let same: Vec<_> = f
        .instances()
        .iter()
        .filter(|i| f.definition(&i.definition).unwrap().symbol.name == "same")
        .collect();
    assert_eq!(same.len(), 2);
    assert_ne!(same[0].definition, same[1].definition);
}

#[test]
fn argument_slots_capacity_record_order_and_assignment_are_explicit() {
    let mapped=checked("type P = { a:i32; b:i32 }\nfn inspect(read x) { x }\nfn bump(inout x) { x <- x+1 }\nfn main() { let mut p=record P { b=0; a=40 }; bump(inout p.a); let mut n=inspect(p.a); bump(inout n); inspect(n) + inspect(1+1) - bytes_len(bytes_new(2)) }");
    let arguments: Vec<_> = mapped
        .nodes()
        .iter()
        .filter_map(|n| match mapped.typed_node(&n.id) {
            Some(TypedNodeRef::Argument(a)) => Some((n, a)),
            _ => None,
        })
        .collect();
    for (node, arg) in &arguments {
        if matches!(
            arg,
            typed::CallArg::ReadCopy(_)
                | typed::CallArg::InoutCopy(_)
                | typed::CallArg::ReadField(_)
                | typed::CallArg::InoutField(_)
        ) {
            assert!(mapped.children(&node.id).unwrap().is_empty());
        }
    }
    assert!(arguments
        .iter()
        .any(|(_, a)| matches!(a, typed::CallArg::ReadTemporary(_))));
    let capacity = mapped
        .nodes()
        .iter()
        .find(|n| n.role == TypedChildRole::Capacity)
        .unwrap();
    assert!(matches!(
        mapped.typed_node(&capacity.id),
        Some(TypedNodeRef::Capacity(2))
    ));
    let NodeOrigin::SourceDerived {
        source,
        rule: DirectRule::EmbeddedCapacity,
    } = &capacity.origin
    else {
        panic!("capacity provenance")
    };
    assert!(
        matches!(mapped.source_representation(&capacity.instance,source).unwrap(),SourceRepresentation::Represented(rows) if rows.contains(&SourceDisposition::EmbeddedOperand(capacity.id.clone())))
    );
    let fields: Vec<_> = mapped
        .nodes()
        .iter()
        .filter_map(|n| match mapped.typed_node(&n.id) {
            Some(TypedNodeRef::RecordField {
                written,
                declaration,
                ..
            }) => Some((written, declaration)),
            _ => None,
        })
        .collect();
    assert_eq!(fields, [(0, 1), (1, 0)]);
}

#[test]
fn nested_patterns_literal_wildcard_and_arm_binders_have_real_slots() {
    let mapped=checked("type O<a> = No | Yes(a)\nfn main() { match Yes(Yes(42)) { Yes(Yes(x)) => x | Yes(No) => 1 | No => 0 } }");
    assert_eq!(
        mapped
            .nodes()
            .iter()
            .filter(|n| matches!(mapped.typed_node(&n.id), Some(TypedNodeRef::Arm(_))))
            .count(),
        3
    );
    assert_eq!(
        mapped
            .nodes()
            .iter()
            .filter(|n| n.role == TypedChildRole::PatternField(0))
            .count(),
        3
    );
    checked("fn main() { match 42 { 0 => 0 | x => x } }");
    checked("fn main() { match true { true => 42 | _ => 0 } }");
}

const CAPTURE:&str="type Mark<a> = Mark(i32)\nfn ints(x:Mark<i32>) { match x { Mark(n) => n } }\nfn bools(x:Mark<bool>) { match x { Mark(n) => n } }\nfn main() { let mut count=0; let snapshot={ count <- count+1; count }; let p=Mark(snapshot); let alias=p; count <- 20; ints(alias)+bools(p)+count*2 }";
#[test]
fn capture_evaluations_and_each_generated_use_are_distinct() {
    let mapped = checked(CAPTURE);
    assert_eq!(typed::evaluate(mapped.typed()).unwrap().to_string(), "42");
    let captures: Vec<_> = mapped
        .shapes()
        .iter()
        .filter_map(|s| match &s.kind {
            ShapeKind::Capture { evaluation } => Some(evaluation),
            _ => None,
        })
        .collect();
    assert_eq!(captures.len(), 1);
    assert!(mapped.direct_source(captures[0]).is_some());
    let uses: Vec<_> = mapped
        .nodes()
        .iter()
        .filter(|n| {
            matches!(
                n.origin,
                NodeOrigin::Generated {
                    reason: GenerationReason::ShapeMaterialization,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(uses.len(), 2);
    assert_ne!(uses[0].id, uses[1].id);
    for node in uses {
        assert!(mapped.direct_source(&node.id).is_none());
        let NodeOrigin::Generated { related, .. } = &node.origin else {
            unreachable!()
        };
        let relations = mapped.relations(related).unwrap();
        assert!(relations
            .iter()
            .any(|r| matches!(r.target, RelationTarget::Shape(_))));
        assert!(relations
            .iter()
            .any(|r| matches!(r.target, RelationTarget::Source(_))));
    }
}

#[test]
fn erased_shapes_keep_continuation_origin_and_share_alias_dags() {
    let mapped =
        checked("type O<a> = None | Some(a)\nfn main() { let a=None; let b=a; let c=Some(b); 42 }");
    let literal = mapped
        .nodes()
        .iter()
        .find(|n| {
            matches!(
                mapped.typed_node(&n.id),
                Some(TypedNodeRef::Expression(typed::ExprNode {
                    kind: typed::ExprKind::Int(42),
                    ..
                }))
            )
        })
        .unwrap();
    assert_eq!(
        mapped
            .direct_source(&literal.id)
            .unwrap()
            .path
            .steps
            .iter()
            .filter(|s| **s == AstStep::LetBody)
            .count(),
        3
    );
    assert!(mapped.nodes().iter().all(|n| !matches!(
        mapped.typed_node(&n.id),
        Some(TypedNodeRef::Expression(typed::ExprNode {
            kind: typed::ExprKind::Let { .. },
            ..
        }))
    )));
    let mut source =
        "type O<a> = None | Some(a)\ntype Pair<a,b> = Pair(a,b)\nfn main() { let x0=None;"
            .to_string();
    for i in 1..=12 {
        source += &format!("let x{i}=Pair(x{},x{});", i - 1, i - 1);
    }
    source += "42 }";
    let shared = checked(&source);
    assert_eq!(shared.shapes().len(), 13);
    checked(
        "type O<a> = None | Some(a)\nfn main() { let a=None; let b=Some(a); let c=Some(b); 42 }",
    );
}

#[test]
fn shape_record_orders_generalized_loans_and_capture_projection_temporary() {
    for fields in ["fixed=2; value=None", "value=None; fixed=2"] {
        checked(&format!("type O<a> = None | Some(a)\ntype R<a> = {{fixed:i32;value:O<a>}}\nfn ints(read x:O<i32>) {{ match x {{ None=>40 | Some(n)=>n }} }}\nfn bools(read x:O<bool>) {{ 0 }}\nfn main() {{ let r=record R {{ {fields} }}; ints(r.value)+bools(r.value)+r.fixed }}"));
    }
    let mapped=checked("type Cell = { n:i32 }\ntype R<a> = { fixed:Cell; marker:a }\ntype O<a> = None | Some(a)\nfn main() { let mut c=record Cell {n=42}; let snapshot=c; let p=record R {fixed=snapshot;marker=None}; c.n <- 0; p.fixed.n }");
    assert!(mapped.nodes().iter().any(|n| matches!(
        n.origin,
        NodeOrigin::Generated {
            reason: GenerationReason::ProjectedCaptureTemporary,
            ..
        }
    )));
    assert!(mapped.nodes().iter().any(|n| matches!(
        n.origin,
        NodeOrigin::SourceDerived {
            rule: DirectRule::ProjectedUse,
            ..
        }
    )));
}

#[test]
fn copy_scope_does_not_narrow_old_owner_index_or_ordinary_native_behavior() {
    let owner="fn keep(owner) { let n=box_read(owner); move owner }\nfn main() { let owner=system_box_i32_abort(42); let returned=keep(move owner); box_read(returned) }";
    let fixture = Fixture::new(owner);
    let old = lower_main_with_function_origins(fixture.sources()).unwrap();
    assert_eq!(typed::evaluate(old.typed()).unwrap().to_string(), "42");
    assert!(lower_main_with_copy_node_origins(fixture.sources())
        .unwrap_err()
        .contains("checked owner obligations"));
    let fixture = Fixture::new("extern fn unused(x:i32)->i32; fn main() { 42 }");
    let sources = fixture.sources();
    let checked = check::analyze(sources.program()).unwrap();
    assert!(typed::lower_main(&checked).is_ok());
    assert!(lower_main_with_copy_node_origins(sources)
        .unwrap_err()
        .contains("does not map native"));
}

#[test]
fn mapped_c_executes_identically_at_o0_and_o2() {
    for source in [
        CAPTURE,
        "fn main() { let b=bytes_set(bytes_new(4),1,42); bytes_get(b,1) }",
        "type O<a> = None | Some(a)\nfn main() { match Some(42) { Some(n)=>n | None=>0 } }",
    ] {
        let fixture = Fixture::new(source);
        let mapped = checked(source);
        let c = typed::emit_c(mapped.typed()).unwrap();
        let path = fixture.0.join("output.c");
        let binary = fixture.0.join("output");
        fs::write(&path, c).unwrap();
        for opt in ["-O0", "-O2"] {
            let output = Command::new("cc")
                .args(["-std=c11", "-pedantic-errors", opt])
                .arg(&path)
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
            assert!(output.status.success());
            assert_eq!(output.stdout, b"42\n");
        }
    }
}

#[test]
fn sixteen_source_steps_are_separate_from_internal_capture_projection() {
    let mut source = "type O<a> = None | Some(a)\ntype R0 = { value:i32 }\n".to_string();
    let mut value = "record R0 {value=42}".to_string();
    for index in 1..=14 {
        source += &format!("type R{index} = {{ value:R{} }}\n", index - 1);
        value = format!("record R{index} {{value={value}}}");
    }
    source+=&format!("type Root<a> = {{ saved:R14; marker:O<a> }}\nfn main() {{ let snapshot={value}; let p=record Root {{marker=None;saved=snapshot}}; p.saved{} }}",".value".repeat(15));
    let mapped = checked(&source);
    assert_eq!(typed::evaluate(mapped.typed()).unwrap().to_string(), "42");
    let projected = mapped
        .nodes()
        .iter()
        .find(|n| {
            matches!(
                n.origin,
                NodeOrigin::SourceDerived {
                    rule: DirectRule::ProjectedUse,
                    ..
                }
            )
        })
        .unwrap();
    let Some(TypedNodeRef::Expression(typed::ExprNode {
        kind: typed::ExprKind::FieldRead(place),
        ..
    })) = mapped.typed_node(&projected.id)
    else {
        panic!("projected field")
    };
    assert_eq!(place.path.len(), 15);
}

#[test]
fn vectors_and_short_circuit_traps_preserve_ordinary_results_and_c() {
    checked(include_str!("fixtures/local-polymorphic-vector.rill"));
    checked("fn main() { if false && (1/0==0) { 0 } else { 42 } }");
    checked("fn main() { let mut n=0; while n<3 {n <- n+1;}; n+39 }");
}

#[test]
fn equal_capture_layouts_preserve_shadowed_and_instance_local_roots() {
    let mapped=checked("type M<a> = M(i32)\nfn ints(x:M<i32>){match x{M(n)=>n}}\nfn bools(x:M<bool>){match x{M(n)=>n}}\nfn main(){let p=M(20);let old=p;let p=M(1);ints(old)+bools(old)+ints(p)+bools(p)}");
    let environments: Vec<_> = mapped
        .nodes()
        .iter()
        .filter_map(|n| {
            if !matches!(
                n.origin,
                NodeOrigin::Generated {
                    reason: GenerationReason::CaptureEnvironmentBinding,
                    ..
                }
            ) {
                return None;
            }
            match mapped.typed_node(&n.id) {
                Some(TypedNodeRef::Expression(typed::ExprNode {
                    kind: typed::ExprKind::Let { local, value, .. },
                    ..
                })) => {
                    let typed::ExprKind::Record { adt, .. } = &value.kind else {
                        panic!("capture record")
                    };
                    Some((*local, *adt))
                }
                _ => panic!("capture binding"),
            }
        })
        .collect();
    assert_eq!(environments.len(), 2);
    assert_ne!(environments[0].0, environments[1].0);
    assert_eq!(environments[0].1, environments[1].1);
    checked("type M<a,b> =M(a)\nfn keep(x:M<i32,bool>){match x{M(n)=>n}}\nfn flag(x:M<bool,i32>){match x{M(n)=>n}}\nfn f(x){let p=M(x);p}\nfn main(){if flag(f(true)){keep(f(42))}else{0}}");
}

#[test]
fn unreachable_owner_declaration_is_a_conservative_new_api_boundary() {
    let fixture = Fixture::new(
        "fn unused(){let owner=system_box_i32_abort(1);box_read(owner)}\nfn main(){42}",
    );
    let old = lower_main_with_function_origins(fixture.sources()).unwrap();
    assert_eq!(typed::evaluate(old.typed()).unwrap().to_string(), "42");
    assert!(lower_main_with_copy_node_origins(fixture.sources())
        .unwrap_err()
        .contains("checked owner obligations"));
}

#[test]
fn source_only_annotations_erased_bindings_and_outside_coverage_are_queryable() {
    let mapped=checked("type O<a> = None|Some(a)\nfn unused(x:i32){x}\nfn typed(x:i32){x}\nfn main(){let none=None;let alias=none;typed(42)}");
    let main = &mapped.function_origins().entry();
    let mut erased = 0;
    let mut annotations = 0;
    let mut aliases = 0;
    let mut outside = 0;
    assert_eq!(mapped.source_nodes().len(), mapped.sources().nodes().len());
    for (source, node) in mapped.source_nodes() {
        assert!(std::ptr::eq(
            mapped.function_origins().source_node(&source).unwrap(),
            node
        ));
        let query = mapped
            .function_origins()
            .instances()
            .iter()
            .map(|i| mapped.source_representation(&i.id, &source))
            .find(|result| !matches!(result, Err(OriginQueryError::WrongFunction)))
            .unwrap_or(Err(OriginQueryError::WrongFunction));
        match query {
            Ok(SourceRepresentation::Represented(rows)) => {
                for row in rows {
                    match row {
                        SourceDisposition::StaticShapeOnly { .. } => erased += 1,
                        SourceDisposition::SignatureOnly => annotations += 1,
                        SourceDisposition::AliasOfShape { .. } => aliases += 1,
                        _ => {}
                    }
                }
            }
            Ok(SourceRepresentation::OutsideCoverage) => outside += 1,
            Err(OriginQueryError::WrongFunction) => {}
            result => panic!("unexpected source query {result:?}"),
        }
    }
    assert_eq!(erased, 2);
    assert!(annotations > 0);
    assert!(aliases > 0);
    assert!(outside > 0);
    let independent = Fixture::new("fn main(){42}").mapped();
    let (foreign, _) = independent.source_nodes().next().unwrap();
    assert_eq!(
        mapped.source_representation(main, &foreign),
        Err(OriginQueryError::ForeignSource)
    );
}

#[test]
fn f2_raw_native_metadata_and_operations_keep_the_explicit_mapping_boundary() {
    let mut cases = vec![
        "fn main(){u8(42)}".to_string(),
        "fn main(){u32(42)}".to_string(),
        "fn main(){u64(42)}".to_string(),
        "fn main(){f32(42)}".to_string(),
        "fn take(x:ptr<void>){is_null(x)} fn main(){take(c_null())}".to_string(),
        "extern fn unused(x:ptr<void>)->u32; fn main(){42}".to_string(),
        "extern fn text(x:cstr)->i32; fn main(){text(c\"hello\")}".to_string(),
    ];
    for raw in ["u8", "u32", "u64", "f32", "ptr<void>", "const_ptr<void>"] {
        cases.push(format!("type E = Empty|Has({raw}) fn main(){{Empty}}"));
        cases.push(format!(
            "type Marker<a> = Marker fn take(x:Marker<{raw}>){{42}} fn main(){{take(Marker)}}"
        ));
    }
    for text in cases {
        let fixture = Fixture::new(&text);
        let sources = fixture.sources();
        let checked = check::analyze(sources.program()).unwrap();
        let ordinary = typed::lower_main(&checked).unwrap();
        let c = typed::emit_c(&ordinary).unwrap();
        assert_eq!(rill::backend::emit(sources.program()).unwrap(), c);
        let expected = typed::evaluate(&ordinary);
        assert_eq!(interpreter::run_reference(sources.program()), expected);
        let error = lower_main_with_copy_node_origins(fixture.sources()).unwrap_err();
        assert!(
            error.starts_with("E-TYPED-UNSUPPORTED") && error.contains("does not map native"),
            "{text}: {error}"
        );
        let error = lower_main_with_function_origins(fixture.sources()).unwrap_err();
        assert!(
            error.starts_with("E-TYPED-UNSUPPORTED") && error.contains("does not map native"),
            "{text}: {error}"
        );
        assert_eq!(
            typed::emit_c(&typed::lower_main(&checked).unwrap()).unwrap(),
            c
        );
    }
}
