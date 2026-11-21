//! Validate explicit nominal identities before intrinsic type inference.
use crate::ast::{Program, TypeDecl, TypeExpr};
use std::collections::HashSet;

pub(super) fn validate(program: &Program) -> Result<(), String> {
    let Some(m) = &program.allocation_types else {
        return Ok(());
    };
    let invalid = || "E-ALLOC: invalid allocation type metadata".to_string();
    let names = [
        &m.result_type,
        &m.error_type,
        &m.ok_constructor,
        &m.err_constructor,
        &m.invalid_length_constructor,
        &m.size_overflow_constructor,
        &m.out_of_memory_constructor,
    ];
    if names.iter().any(|name| name.is_empty())
        || m.result_type == m.error_type
        || names[2..].iter().collect::<HashSet<_>>().len() != 5
    {
        return Err(invalid());
    }
    let declaration = |name: &str| -> Result<&TypeDecl, String> {
        let mut matches = program.types.iter().filter(|decl| decl.name == name);
        let decl = matches.next().ok_or_else(invalid)?;
        if matches.next().is_some() {
            return Err(invalid());
        }
        Ok(decl)
    };
    let result = declaration(&m.result_type)?;
    let error = declaration(&m.error_type)?;
    if result.record_fields.is_some()
        || result.params.len() != 2
        || result.params[0] == result.params[1]
        || result.variants.len() != 2
        || error.record_fields.is_some()
        || !error.params.is_empty()
        || error.variants.len() != 3
    {
        return Err(invalid());
    }
    let contracts = [
        (
            result,
            &m.ok_constructor,
            vec![TypeExpr {
                name: result.params[0].clone(),
                args: vec![],
            }],
        ),
        (
            result,
            &m.err_constructor,
            vec![TypeExpr {
                name: result.params[1].clone(),
                args: vec![],
            }],
        ),
        (error, &m.invalid_length_constructor, vec![]),
        (error, &m.size_overflow_constructor, vec![]),
        (error, &m.out_of_memory_constructor, vec![]),
    ];
    for (owner, name, fields) in contracts {
        let mut variants = program
            .types
            .iter()
            .flat_map(|decl| decl.variants.iter().map(move |v| (decl, v)))
            .filter(|(_, variant)| &variant.name == name);
        let (decl, variant) = variants.next().ok_or_else(invalid)?;
        if variants.next().is_some() || !std::ptr::eq(owner, decl) || variant.fields != fields {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::AllocationTypes;
    fn fixture() -> Program {
        let mut p = crate::parser::parse("type Result<a, b> = Err(b) | Ok(a)\ntype AllocError = OutOfMemory | InvalidLength | SizeOverflow\nlet main = 42").unwrap();
        p.allocation_types = Some(AllocationTypes {
            result_type: "Result".into(),
            ok_constructor: "Ok".into(),
            err_constructor: "Err".into(),
            error_type: "AllocError".into(),
            invalid_length_constructor: "InvalidLength".into(),
            size_overflow_constructor: "SizeOverflow".into(),
            out_of_memory_constructor: "OutOfMemory".into(),
        });
        p
    }
    #[test]
    fn accepts_named_variants_in_any_order() {
        crate::check::check(&fixture()).unwrap();
    }
    #[test]
    fn rejects_corrupt_metadata_and_shapes() {
        let changes: Vec<fn(&mut Program)> = vec![
            |p| p.allocation_types.as_mut().unwrap().result_type.clear(),
            |p| p.allocation_types.as_mut().unwrap().error_type = "Result".into(),
            |p| p.allocation_types.as_mut().unwrap().ok_constructor = "Err".into(),
            |p| {
                p.allocation_types
                    .as_mut()
                    .unwrap()
                    .out_of_memory_constructor = "Missing".into()
            },
            |p| p.types[0].params[1] = "a".into(),
            |p| p.types[0].params.pop().map(|_| ()).unwrap(),
            |p| p.types[0].record_fields = Some(vec![]),
            |p| p.types[1].record_fields = Some(vec![]),
            |p| p.types[1].params.push("e".into()),
            |p| p.types[0].variants[0].fields[0].name = "a".into(),
            |p| {
                p.types[0].variants[1].fields[0].args.push(TypeExpr {
                    name: "a".into(),
                    args: vec![],
                })
            },
            |p| {
                p.types[1].variants[0].fields.push(TypeExpr {
                    name: "i32".into(),
                    args: vec![],
                })
            },
            |p| {
                let v = p.types[1].variants[0].clone();
                p.types[1].variants.push(v);
            },
            |p| {
                p.types[1].variants.pop();
            },
            |p| {
                let d = p.types[0].clone();
                p.types.push(d);
            },
        ];
        for change in changes {
            let mut p = fixture();
            change(&mut p);
            assert!(validate(&p).is_err(), "accepted {p:?}");
        }
    }
    #[test]
    fn source_only_fallible_calls_require_import() {
        for name in ["system_box_i32", "system_bytes_zeroed"] {
            let p =
                crate::parser::parse(&format!("let main = let outcome = {name}(1) in 42")).unwrap();
            assert!(crate::check::check(&p)
                .unwrap_err()
                .contains("import std.alloc"));
        }
    }
}
