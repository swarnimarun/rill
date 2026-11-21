//! Deferred nominal identity is solved within a monomorphic component, while
//! ordinary value restriction, per-use identity and source loan checks remain.
use rill::{ast::Expr, backend, check, interpreter, parser, typed};
use std::collections::HashMap;
#[path = "support/deferred_projection_cases.rs"]
mod cases;

type Consumer = fn(&rill::ast::Program) -> Result<(), String>;
const CONSUMERS: [Consumer; 7] = [
    check::check,
    |p| check::analyze(p).map(|_| ()),
    |p| check::analyze(p).and_then(|checked| typed::lower_main(&checked).map(|_| ())),
    |p| interpreter::run(p).map(|_| ()),
    |p| interpreter::run_reference(p).map(|_| ()),
    |p| backend::emit(p).map(|_| ()),
    |p| backend::emit_reference(p).map(|_| ()),
];

fn reject(source: &str, diagnostic: &str) {
    let program = parser::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    for (index, consumer) in CONSUMERS.into_iter().enumerate() {
        let error = match consumer(&program) {
            Ok(()) => panic!("consumer {index} accepted {source}"),
            Err(error) => error,
        };
        assert!(
            error.starts_with(diagnostic),
            "consumer {index}, expected {diagnostic}: {error}\n{source}"
        );
    }
}

// Visit the actual AST, never a clone: FieldInfo is keyed by source occurrence.
fn projections<'a>(expression: &'a Expr, found: &mut Vec<(&'a Expr, &'a [String])>) {
    match expression {
        Expr::Field(_, path) | Expr::InoutField(_, path) => found.push((expression, path)),
        Expr::FieldAssign(_, path, value) => {
            found.push((expression, path));
            projections(value, found);
        }
        Expr::Record(_, fields) => {
            for (_, value) in fields {
                projections(value, found);
            }
        }
        Expr::Binary(_, a, b) | Expr::While(a, b) | Expr::Let(_, a, b) | Expr::LetMut(_, a, b) => {
            projections(a, found);
            projections(b, found);
        }
        Expr::If(a, b, c) => {
            projections(a, found);
            projections(b, found);
            projections(c, found);
        }
        Expr::Assign(_, value) | Expr::Lambda(_, value) => projections(value, found),
        Expr::FunctionOp(_, a, b) => {
            projections(a, found);
            projections(b, found);
        }
        Expr::Apply(callee, arguments) => {
            projections(callee, found);
            for argument in arguments {
                projections(argument, found);
            }
        }
        Expr::Call(_, arguments) => {
            for argument in arguments {
                projections(argument, found);
            }
        }
        Expr::Match(value, arms) => {
            projections(value, found);
            for arm in arms {
                projections(&arm.body, found);
            }
        }
        Expr::Int(_)
        | Expr::Str(_)
        | Expr::CStr(_)
        | Expr::Bool(_)
        | Expr::Unit
        | Expr::Var(_)
        | Expr::Move(_)
        | Expr::Inout(_) => {}
    }
}

fn verify_source_projection_metadata(checked: &check::CheckedProgram<'_>) {
    let mut occurrences = Vec::new();
    for function in &checked.program.functions {
        projections(&function.body, &mut occurrences);
    }
    assert!(!occurrences.is_empty());
    for (expression, path) in occurrences {
        let info = checked
            .field_info(expression)
            .unwrap_or_else(|| panic!("missing canonical field metadata: {expression:?}"));
        assert_eq!(info.len(), path.len(), "{expression:?}");
        assert!(!info.is_empty(), "unresolved metadata escaped checking");
        assert!(checked.expression_type(expression).is_some());
        for (step, label) in info.iter().zip(path) {
            let declaration = checked
                .program
                .types
                .iter()
                .find(|declaration| declaration.name == step.record)
                .unwrap_or_else(|| panic!("unknown nominal identity {}", step.record));
            let labels = declaration.record_fields.as_ref().unwrap();
            assert_eq!(step.arity, labels.len(), "{expression:?}");
            assert_eq!(labels.get(step.index), Some(label), "{expression:?}");
        }
    }
}

#[test]
fn deferred_paths_agree_across_checked_typed_and_reference_consumers() {
    for case in cases::cases() {
        let program = parser::parse(&case.source).unwrap_or_else(|e| panic!("{}: {e}", case.label));
        check::check(&program).unwrap_or_else(|e| panic!("{}: {e}", case.label));
        let checked = check::analyze(&program).unwrap_or_else(|e| panic!("{}: {e}", case.label));
        verify_source_projection_metadata(&checked);
        let lowered = typed::lower_main(&checked).unwrap_or_else(|e| panic!("{}: {e}", case.label));
        typed::verify(&lowered).unwrap();
        let reference = interpreter::run_reference(&program);
        match case.expected {
            Some(expected) => assert_eq!(
                reference.as_ref().unwrap().to_string(),
                expected,
                "{}",
                case.label
            ),
            None => assert!(reference.is_err(), "{}", case.label),
        }
        assert_eq!(typed::evaluate(&lowered), reference, "{}", case.label);
        assert_eq!(interpreter::run(&program), reference, "{}", case.label);
        let c = typed::emit_c(&lowered).unwrap();
        assert_eq!(backend::emit(&program).unwrap(), c, "{}", case.label);
        assert!(!c.contains("rl_value") && !c.contains("RL_CAP") && !c.contains("restrict"));
        let reference_c = backend::emit_reference(&program);
        if case.reference_c {
            reference_c.unwrap_or_else(|e| panic!("{}: {e}", case.label));
        } else {
            assert_eq!(
                reference_c.unwrap_err(),
                "E-C-ADT: unresolved polymorphic value requires local specialization",
                "{}",
                case.label
            );
        }
    }
}

fn canonical_type(ty: &check::Type, ids: &mut HashMap<usize, usize>) -> check::Type {
    match ty {
        check::Type::Var(id) => {
            let next = ids.len();
            check::Type::Var(*ids.entry(*id).or_insert(next))
        }
        check::Type::Nominal(name, arguments) => check::Type::Nominal(
            name.clone(),
            arguments.iter().map(|ty| canonical_type(ty, ids)).collect(),
        ),
        atom => atom.clone(),
    }
}

fn canonical_signature(
    signature: &check::FunctionType,
) -> (Vec<usize>, Vec<check::Type>, check::Type) {
    assert!(signature
        .quantified
        .windows(2)
        .all(|pair| pair[0] < pair[1]));
    let mut ids = HashMap::new();
    let params = signature
        .params
        .iter()
        .map(|ty| canonical_type(ty, &mut ids))
        .collect();
    let result = canonical_type(&signature.result, &mut ids);
    // Include membership, not just the number of quantifiers. Extra quantifiers
    // absent from the signature are represented too, rather than discarded.
    let mut quantified = signature
        .quantified
        .iter()
        .map(|id| {
            let next = ids.len();
            *ids.entry(*id).or_insert(next)
        })
        .collect::<Vec<_>>();
    quantified.sort_unstable();
    (quantified, params, result)
}

#[test]
fn resolved_sccs_recover_exact_quantifier_membership_in_both_orders() {
    use check::Type::{Int, Nominal, Var};
    for case in cases::cases().into_iter().filter(|case| {
        case.label.starts_with("generic-")
            || case.label.starts_with("unrelated-none-")
            || case.label.starts_with("nested-path-")
            || case.label.starts_with("nested-occurrences-")
            || case.label.starts_with("selective-pair-")
    }) {
        let program = parser::parse(&case.source).unwrap();
        let checked = check::analyze(&program).unwrap();
        let (quantified, params, result, record_result) =
            if case.label.starts_with("generic-substitution-") {
                (
                    vec![0, 1],
                    vec![Var(0), Var(1)],
                    Var(0),
                    Nominal(
                        "R".into(),
                        vec![Var(1), Var(0), Nominal("O".into(), vec![Var(0)])],
                    ),
                )
            } else if case.label.starts_with("selective-pair-") {
                let payload = Nominal("O".into(), vec![Int]);
                (
                    vec![],
                    vec![payload.clone()],
                    payload.clone(),
                    Nominal("R".into(), vec![payload]),
                )
            } else {
                (
                    vec![0],
                    vec![Var(0)],
                    Var(0),
                    Nominal(
                        if case.label.starts_with("nested-") {
                            "Outer"
                        } else {
                            "R"
                        }
                        .into(),
                        vec![Var(0)],
                    ),
                )
            };
        assert_eq!(
            canonical_signature(&checked.functions["f"]),
            (quantified.clone(), params.clone(), result),
            "{} f",
            case.label
        );
        assert_eq!(
            canonical_signature(&checked.functions["g"]),
            (quantified, params, record_result),
            "{} g",
            case.label
        );
        assert_eq!(
            canonical_signature(&checked.functions["main"]),
            (vec![], vec![], Int),
            "{} main",
            case.label
        );
    }
}

#[test]
fn projection_aliases_and_enclosing_record_or_enum_values_stay_weak() {
    let types = "type O<a> = None|Some(a) type R<a> = {value:a} type H<a> = {value:a} type Hold<a> = Hold(a) type Pair<a,b> = Pair(a,b)";
    let g = "fn g(x){if true{record R{value=None}}else{record R{value=f(x)}}}";
    for body in [
        "let value=r.value;let alias=value;let ints=if true{alias}else{Some(1)};if true{alias}else{Some(true)}",
        "let holder=record H{value=r.value};let alias=holder;let ints=if true{alias.value}else{Some(1)};if true{alias.value}else{Some(true)}",
        "let holder=Hold(r.value);let alias=holder;let ints=match alias{Hold(v)=>if true{v}else{Some(1)}};match alias{Hold(v)=>if true{v}else{Some(true)}}",
        "let value=r.value;let alias=value;let pair=Pair(alias,None);let ints=if true{pair}else{Pair(Some(1),Some(1))};let bools=if true{pair}else{Pair(Some(true),Some(true))};alias",
    ] {
        let f = format!("fn f(x){{let r=g(x);{body}}}");
        for functions in [format!("{f}\n{g}"), format!("{g}\n{f}")] {
            reject(&format!("{types}\n{functions}\nfn main(){{42}}"), "E-TYPE:");
        }
    }
}

#[test]
fn an_unknown_nominal_root_cannot_be_inferred_from_labels_or_later_callers() {
    for declarations in [
        "type R<a> = {value:a}",
        "type R<a> = {value:a} type Other<a> = {value:a}",
    ] {
        reject(
            &format!("{declarations} fn project(p){{p.value}} fn main(){{project(record R{{value=42}})}}"),
            "E-RECORD:",
        );
        reject(
            &format!("{declarations} fn project(p){{let value=p.value;if false{{project(p)}}else{{value}}}} fn main(){{42}}"),
            "E-RECORD:",
        );
    }
    reject(
        "type R<a> = {value:a} fn f(p){let value=p.value;g(p);value} fn g(p){f(p)} fn main(){42}",
        "E-RECORD:",
    );
}

#[test]
fn known_nonrecords_missing_fields_nominal_mismatch_and_occurs_checks_reject() {
    for source in [
        "fn f(p){let value=p.x;let known=if true{p}else{0};value} fn main(){42}",
        "type E=E fn f(p){let value=p.x;let known=if true{p}else{E};value} fn main(){42}",
        "type R={value:i32} fn f(p){let value=p.missing;let known=if true{p}else{record R{value=0}};value} fn main(){42}",
        "type R={value:i32} fn f(p){let value=p.value.missing;let known=if true{p}else{record R{value=0}};value} fn main(){42}",
    ] {
        reject(source, "E-RECORD:");
    }
    for source in [
        "type A={value:i32} type B={value:i32} fn f(p){let value=p.value;let a=if true{p}else{record A{value=0}};let b=if true{p}else{record B{value=0}};value} fn main(){42}",
        "type R<a> = {value:a} fn f(p){let value=p.value;let same=if true{p}else{record R{value=p}};value} fn main(){42}",
        "type R<a> = {value:a} fn f(p,q){let x=p.value;let y=q.value;let a=if true{p}else{record R{value=q}};let b=if true{q}else{record R{value=x}};y} fn main(){42}",
    ] {
        reject(source, "E-TYPE:");
    }
    reject("fn main(){unknown.value}", "E-NAME:");
}

#[test]
fn conflicts_are_detected_before_later_body_equality_can_fix_the_root() {
    let types = "type I={b:i32;a:i32} type Outer={other:i32;inner:I}";
    let helpers = "fn both(inout a,inout b){()} fn shared(read a,inout b){()} fn hold(inout a,b){()} fn hold_read(read a,b){()} fn read_two(read a,read b){()} fn bump(inout a){a<-a+1}";
    // Every fixture reaches its conflicting call with p's nominal root unknown.
    // All helper parameters are unannotated, and scalar arithmetic can constrain
    // a projected result but cannot establish p's declared record identity.
    for bad_call in [
        "both(inout p.inner.a,inout p.inner.a)",
        "both(inout p,inout p.inner.a)",
        "both(inout p.inner.a,inout p)",
        "both(inout p.inner,inout p.inner.a)",
        "both(inout p.inner.a,inout p.inner)",
        "shared(p.inner.a,inout p.inner.a)",
        "shared(p.inner,inout p.inner.a)",
        "shared(p,inout p.inner.a)",
        "hold(inout p.inner.a,p.inner.a)",
        "hold(inout p.inner.a,p.inner)",
        "hold(inout p.inner.a,{p.inner.a<-1;0})",
        "hold(inout p.inner.a,{p.inner<-record I{b=0;a=1};0})",
        "hold_read(p.inner.a,{p.inner.a<-1;0})",
        "hold_read(p.inner.a,{bump(inout p.inner.a);0})",
        "hold(inout p.inner.a,{read_two(p.inner.a,p.inner.a);0})",
        "hold(inout p.inner.a,{bump(inout p.inner.a);0})",
    ] {
        reject(
            &format!("{types}\n{helpers}\nfn f(inout p){{{bad_call};let known=if true{{p}}else{{record Outer{{other=0;inner=record I{{b=0;a=0}}}}}};0}}\nfn main(){{42}}"),
            "E-LOAN:",
        );
    }
}

#[test]
fn immutable_polymorphic_nested_roots_do_not_gain_write_or_inout_authority() {
    let prefix = "type O<a> = None|Some(a) type I<a> = {value:O<a>} type R<a> = {inner:I<a>} fn set(inout x){x<-Some(1)}";
    for (body, diagnostic) in [
        ("r.inner.value<-Some(1)", "E-MUT:"),
        ("set(inout r.inner.value)", "E-LOAN:"),
    ] {
        reject(
            &format!(
                "{prefix} fn main(){{let r=record R{{inner=record I{{value=None}}}};{body};42}}"
            ),
            diagnostic,
        );
    }
    reject(
        "type R={value:i32} fn f(read p){p.value<-1;let known=if true{p}else{record R{value=0}};42} fn main(){42}",
        "E-MUT:",
    );
}

#[test]
fn a_full_sixteen_segment_deferred_path_materializes_every_step() {
    let mut source = String::from("type R0={value:i32}\n");
    let mut value = String::from("record R0{value=42}");
    for level in 1..16 {
        source.push_str(&format!("type R{level}={{next:R{}}}\n", level - 1));
        value = format!("record R{level}{{next={value}}}");
    }
    let path = format!("{}.value", ".next".repeat(15));
    source.push_str(&format!("fn f(p){{let answer=p{path};let known=if true{{p}}else{{{value}}};answer}} fn main(){{f({value})}}"));
    let program = parser::parse(&source).unwrap();
    let checked = check::analyze(&program).unwrap();
    verify_source_projection_metadata(&checked);
    let lowered = typed::lower_main(&checked).unwrap();
    typed::verify(&lowered).unwrap();
    assert_eq!(typed::evaluate(&lowered).unwrap().to_string(), "42");
    assert_eq!(
        interpreter::run_reference(&program).unwrap().to_string(),
        "42"
    );
    assert!(parser::parse(&format!("fn main(){{p{}.value}}", ".next".repeat(16))).is_err());
}
