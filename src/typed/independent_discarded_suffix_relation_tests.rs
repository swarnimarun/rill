//! Independent discarded-result verification and retained-source regressions.
use super::*;
use crate::typed::TypedMain;
const SOURCE:&str="type B<a> = BN | BS(a) fn spin(n:i32){spin(n)+1} fn ignore(a,b){42} fn main(){ignore(spin(0),BN)}";
fn fixture() -> (Frozen, TypedMain) {
    let ast = crate::parser::parse(SOURCE).unwrap();
    let checked = crate::check::analyze(&ast).unwrap();
    let (typed, instances) = crate::typed::lower_main_traced(&checked, None, None).unwrap();
    let frozen = SourceFacts::build(&checked)
        .unwrap()
        .freeze(&instances)
        .unwrap();
    frozen.verify_source(&checked).unwrap();
    frozen.verify_structure(&typed).unwrap();
    (frozen, typed)
}
#[test]
fn independent_k4_source_only_suffix_inactive_owner_native_schema_reject() {
    for forbidden in ["SystemBoxI32", "u32"] {
        let (mut frozen, mut typed) = fixture();
        assert!(typed.adts.is_empty());
        assert!(typed
            .functions
            .iter()
            .all(|f| matches!(f.result, crate::typed::ResultRepr::Value(_))));
        let id = frozen
            .witness
            .environment
            .types
            .iter()
            .position(|d| d.name == "B")
            .unwrap();
        frozen.witness.environment.types[id].variants[1].fields[0].name = forbidden.into();
        typed.logical.declarations[id].variants[1].fields[0].name = forbidden.into();
        typed.logical.control = Some(std::sync::Arc::new(frozen));
        let error = crate::typed::verify(&typed).unwrap_err();
        assert_eq!(crate::typed::evaluate(&typed).unwrap_err(), error);
        assert_eq!(crate::typed::emit_c(&typed).unwrap_err(), error);
    }
}
