//! Independent discarded-result verification and retained-source regressions.
use super::*;
const SOURCE:&str="type B<a> = BN | BS(a) fn spin(n:i32){spin(n)+1} fn ignore(a,b){42} fn main(){ignore(spin(0),BN)}";
fn lower() -> TypedMain {
    let ast = crate::parser::parse(SOURCE).unwrap();
    lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn reject(p: &TypedMain) {
    let e = verify(p).unwrap_err();
    assert_eq!(evaluate(p).unwrap_err(), e);
    assert_eq!(emit_c(p).unwrap_err(), e);
}
#[test]
fn independent_k4_source_only_suffix_is_typed_without_discard_tag_or_layout() {
    let p = lower();
    assert!(p.adts.is_empty());
    assert!(p
        .functions
        .iter()
        .all(|f| matches!(f.result, ResultRepr::Value(_))));
    assert!(!p
        .functions
        .iter()
        .any(|f| any_computation(&f.body, &|k| matches!(k, ExprKind::DiscardLet { .. }))));
    verify(&p).unwrap();
    assert!(!p.logical.binders.is_empty());
    let ast = crate::parser::parse(SOURCE).unwrap();
    assert_eq!(crate::interpreter::run_reference(&ast), evaluate(&p));
    assert_eq!(crate::backend::emit(&ast).unwrap(), emit_c(&p).unwrap());
}
#[test]
fn independent_k4_source_only_suffix_missing_extra_foreign_metadata_reject() {
    let original = lower();
    let foreign = lower();
    for action in 0..8 {
        let mut bad = original.clone();
        match action {
            0 => bad.logical.control = None,
            1 => bad.logical.domain = None,
            2 => bad.logical.declarations.clear(),
            3 => bad.logical = foreign.logical.clone(),
            4 => bad.logical.function_scopes.clear(),
            5 => bad.logical.source_functions.clear(),
            6 => bad.logical.scope_sources.clear(),
            _ => {
                let ordinal = bad.logical.binders.iter().filter(|b| b.scope == 0).count();
                bad.logical
                    .binders
                    .push(logical::OpaqueBinding { scope: 0, ordinal });
            }
        }
        reject(&bad);
    }
}
