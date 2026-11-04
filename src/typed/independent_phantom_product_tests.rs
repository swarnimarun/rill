//! Independently authored K2 representation-corruption probes.
//! All mutations stay inside the existing private typed test boundary.
use crate::{check, parser, typed};

fn reject_all(program: &typed::TypedMain, label: &str, logical: bool) {
    let error = typed::verify(program).expect_err(label);
    if logical {
        assert!(error.contains("E-TYPED-LOGICAL"), "{label}: {error}");
    }
    assert!(
        typed::evaluate(program).is_err(),
        "evaluate accepted {label}"
    );
    assert!(typed::emit_c(program).is_err(), "emit_c accepted {label}");
    println!("REJECT {label}: {error}");
}

fn corrupt(program: &typed::TypedMain, source: &str, mode: &str) -> usize {
    let mut count = 0;
    let root = program
        .adts
        .iter()
        .position(|a| a.symbolic.is_some() && a.variants.iter().any(|v| !v.fields.is_empty()))
        .unwrap();
    if mode == "forgery-closed" {
        for name in ["Leaf", "Middle"] {
            let mut forged = program.clone();
            let id = forged.adts.iter().position(|a| a.name == name).unwrap();
            forged.adts[id].variants[0].fields[0] = typed::Type::Bool;
            reject_all(&forged, name, true);
            count += 1;
        }
    } else if mode == "forgery-pointer" {
        let mut forged = program.clone();
        let id = forged.adts.iter().position(|a| a.name == "Fixed").unwrap();
        assert_ne!(
            forged.adts[id].variants[0].fields[0],
            forged.adts[id].variants[0].fields[1]
        );
        forged.adts[id].variants[0].fields.swap(0, 1);
        reject_all(&forged, "closed pointer pointee constness swap", true);
        count += 1;
    } else if mode == "forgery-identity" {
        let fields = &program.adts[root].variants[0].fields;
        assert!(fields.len() >= 2 && fields[0] != fields[1]);
        let mut forged = program.clone();
        forged.adts[root].variants[0].fields.swap(0, 1);
        reject_all(&forged, "equal-layout child identity swap", true);
        count += 1;
    }
    for change in 0..6 {
        let mut forged = program.clone();
        match change {
            0 => forged.adts[root].variants[0].fields.clear(),
            1 => forged.adts[root].variants[0].name.push_str("_forged"),
            2 => forged.adts[root].variants[0].fields[0] = typed::Type::OwnedI32,
            3 => {
                forged.adts[root].variants[0].fields[0] = typed::Type::Adt {
                    id: root,
                    owned: false,
                }
            }
            4 => forged.adts[root].origin = typed::AdtOrigin::LocalCapture,
            5 => {
                let parsed = parser::parse(source).unwrap();
                let checked = check::analyze(&parsed).unwrap();
                let other = typed::lower_main(&checked).unwrap();
                forged.adts[root].symbolic = other.adts[root].symbolic.clone();
            }
            _ => unreachable!(),
        }
        reject_all(
            &forged,
            &format!("generic product corruption {change}"),
            true,
        );
        count += 1;
    }
    count
}

fn probe(source: &str, mode: &str, expected: usize) {
    let parsed = parser::parse(source).unwrap();
    let checked = check::analyze(&parsed).unwrap();
    let program = typed::lower_main(&checked).unwrap();
    typed::verify(&program).unwrap();
    assert_eq!(corrupt(&program, source, mode), expected);
}

#[test]
fn independent_closed_descendants_reject_eight_corruptions() {
    probe("type Leaf = Leaf(i32)\ntype Middle = Middle(Leaf)\ntype Open<a> = Open(Middle)\nfn make() { Open(Middle(Leaf(42))) }\nfn main() { match make() { Open(Middle(Leaf(n))) => n } }", "forgery-closed", 8);
}

#[test]
fn independent_nested_identities_reject_seven_corruptions() {
    probe("type Flag<a> = Off | On\ntype Open<a> = Open(a,Flag<a>)\nfn main() { match Open(On,Off) { Open(On,Off) => 42 | _ => 0 } }", "forgery-identity", 7);
}

#[test]
fn independent_closed_phantom_identities_reject_seven_corruptions() {
    probe("type Flag<a> = Off | On\ntype Open<a> = Open(Flag<i32>,Flag<bool>)\nfn main() { match Open(On,Off) { Open(On,Off) => 42 | _ => 0 } }", "forgery-identity", 7);
}

#[test]
fn independent_closed_pointer_descendants_reject_seven_corruptions() {
    probe("type Fixed = { mutable: ptr<void>; shared: const_ptr<void> }\ntype Open<a> = Open(Fixed)\nfn main() { let p = Open(record Fixed { mutable = c_null(); shared = c_null() }); match p { Open(f) => if is_null(f.mutable) && is_null(f.shared) { 42 } else { 0 } } }", "forgery-pointer", 7);
}
