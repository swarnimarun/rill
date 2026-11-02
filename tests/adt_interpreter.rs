use rill::ast::{Expr, Function, MatchArm, Pattern, Program, TypeDecl, TypeExpr, VariantDecl};
use rill::interpreter;

#[test]
fn nested_constructor_patterns_bind_payloads() {
    let p = Program {
        native: Default::default(),
        constants: vec![],
        allocation_types: None,
        externs: vec![],
        types: vec![TypeDecl {
            record_fields: None,
            name: "Option".into(),
            params: vec!["a".into()],
            variants: vec![
                VariantDecl {
                    name: "None".into(),
                    fields: vec![],
                },
                VariantDecl {
                    name: "Some".into(),
                    fields: vec![TypeExpr {
                        name: "a".into(),
                        args: vec![],
                    }],
                },
            ],
        }],
        functions: vec![Function {
            name: "main".into(),
            params: vec![],
            body: Expr::Match(
                Box::new(Expr::Call(
                    "Some".into(),
                    vec![Expr::Call("Some".into(), vec![Expr::Int(42)])],
                )),
                vec![
                    MatchArm {
                        pattern: Pattern::Constructor(
                            "Some".into(),
                            vec![Pattern::Constructor(
                                "Some".into(),
                                vec![Pattern::Binding("x".into())],
                            )],
                        ),
                        body: Expr::Var("x".into()),
                    },
                    MatchArm {
                        pattern: Pattern::Wildcard,
                        body: Expr::Int(0),
                    },
                ],
            ),
        }],
    };
    assert_eq!(interpreter::run(&p).unwrap().to_string(), "42");
}
