use rill::{backend, interpreter, parser};

#[test]
fn public_execution_and_codegen_reject_unchecked_programs() {
    let cases = [
        ("let main = true + 1", "E-TYPE"),
        ("let main = let x = 0 in x <- 1", "E-MUT"),
        (
            "let use_both (inout x) y = ()\nlet main = let mut x = 1 in use_both(inout x, x)",
            "E-LOAN",
        ),
        (
            "type Option<a> = None | Some(a)\nlet main = match Some(1) { Some(x) => x }",
            "E-MATCH",
        ),
        ("let main = if true then 1 else false", "E-TYPE"),
    ];
    for (source, code) in cases {
        let program = parser::parse(source).unwrap();
        let emit_error = backend::emit(&program).unwrap_err();
        let reference_error = backend::emit_reference(&program).unwrap_err();
        let run_error = interpreter::run(&program).unwrap_err();
        assert!(emit_error.contains(code), "{source}: {emit_error}");
        assert!(
            reference_error.contains(code),
            "{source}: {reference_error}"
        );
        assert!(run_error.contains(code), "{source}: {run_error}");
    }
}

#[test]
fn fabricated_constructor_names_are_rejected_before_c_emission() {
    use rill::ast::{Expr, Function, Program, TypeDecl, VariantDecl};
    for name in ["A\"; bad", "A\0bad", "M$not-a-number$Okay", "M$1$Bad-Name"] {
        let program = Program {
            native: Default::default(),
            constants: vec![],
            allocation_types: None,
            types: vec![TypeDecl {
                record_fields: None,
                name: "T".into(),
                params: vec![],
                variants: vec![VariantDecl {
                    name: name.into(),
                    fields: vec![],
                }],
            }],
            externs: vec![],
            functions: vec![Function {
                name: "main".into(),
                params: vec![],
                body: Expr::Unit,
            }],
        };
        assert!(rill::backend::emit(&program)
            .unwrap_err()
            .contains("E-NAME"));
        assert!(rill::backend::emit_reference(&program)
            .unwrap_err()
            .contains("E-NAME"));
        assert!(rill::interpreter::run(&program)
            .unwrap_err()
            .contains("E-NAME"));
    }
}

#[test]
fn malformed_record_metadata_is_rejected() {
    use rill::ast::{Expr, Function, Program, TypeDecl, TypeExpr, VariantDecl};
    for (record, field) in [("Point\0bad", "x"), ("Point", "x\0bad"), ("Point", "x-y")] {
        let program = Program {
            native: Default::default(),
            constants: vec![],
            allocation_types: None,
            types: vec![TypeDecl {
                name: record.into(),
                params: vec![],
                record_fields: Some(vec![field.into()]),
                variants: vec![VariantDecl {
                    name: "Point".into(),
                    fields: vec![TypeExpr {
                        name: "i32".into(),
                        args: vec![],
                    }],
                }],
            }],
            externs: vec![],
            functions: vec![Function {
                name: "main".into(),
                params: vec![],
                body: Expr::Unit,
            }],
        };
        assert!(rill::backend::emit(&program)
            .unwrap_err()
            .contains("E-RECORD"));
        assert!(rill::backend::emit_reference(&program)
            .unwrap_err()
            .contains("E-RECORD"));
        assert!(rill::interpreter::run(&program)
            .unwrap_err()
            .contains("E-RECORD"));
    }
}

#[test]
fn fabricated_field_paths_respect_checked_limits() {
    use rill::ast::Expr;
    for path in [vec![], vec!["x".to_string(); 17]] {
        let mut program =
            parser::parse("type P = { x: i32 }\nlet main = let p = record P { x = 1 } in p.x")
                .unwrap();
        let Expr::Let(_, _, body) = &mut program.functions[0].body else {
            panic!("expected let")
        };
        **body = Expr::Field("p".into(), path);
        assert!(backend::emit(&program).unwrap_err().contains("E-RECORD"));
        assert!(backend::emit_reference(&program)
            .unwrap_err()
            .contains("E-RECORD"));
        assert!(interpreter::run(&program).unwrap_err().contains("E-RECORD"));
    }
    let mut program = parser::parse("type P = { x: i32 }\nlet f (inout x) = ()\nlet main = let mut p = record P { x = 1 } in f(inout p.x)").unwrap();
    let Expr::LetMut(_, _, body) = &mut program.functions[1].body else {
        panic!("expected mutable let")
    };
    let Expr::Call(_, args) = body.as_mut() else {
        panic!("expected call")
    };
    args[0] = Expr::InoutField("p".into(), vec![]);
    assert!(backend::emit(&program).unwrap_err().contains("E-RECORD"));
    assert!(backend::emit_reference(&program)
        .unwrap_err()
        .contains("E-RECORD"));
    assert!(interpreter::run(&program).unwrap_err().contains("E-RECORD"));
}
