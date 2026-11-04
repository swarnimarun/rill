use rill::{
    ast::{BinaryOp, Expr, FunctionOp, ParamMode},
    formatter, parser,
    source_map::{AstNode, AstPath, AstStep},
    syntax::{AstDeclaration, SyntaxElement, SyntaxKind, SyntaxTree},
};

fn expression(source: &str) -> Expr {
    parser::parse(&format!("fn main() {{ {source} }}"))
        .unwrap()
        .functions
        .remove(0)
        .body
}

fn source_tree(source: &str) -> SyntaxTree<'_> {
    let tree = SyntaxTree::parse(source);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    assert_eq!(
        tree.parsed_module(),
        Some(&parser::parse_module(source).unwrap())
    );
    let map = tree.ast_source_map().unwrap();
    for entry in map.entries() {
        assert!(entry.path.resolve(tree.parsed_module().unwrap()).is_some());
    }
    let mut work = vec![SyntaxElement::Node(tree.root())];
    let mut leaves = Vec::new();
    while let Some(element) = work.pop() {
        match element {
            SyntaxElement::Node(node) => {
                work.extend(tree.node(node).unwrap().children().iter().rev())
            }
            SyntaxElement::Lexeme(index) => leaves.push(index),
        }
    }
    assert_eq!(
        leaves,
        (0..tree.source().lexemes().len()).collect::<Vec<_>>()
    );
    assert_eq!(tree.reconstruct(), source);
    tree
}

#[test]
fn lambdas_reuse_parameter_modes_annotations_and_braced_bodies() {
    let source = "fn main() { fn(read x: Option<Option<i32>>, inout y, z) { z } }";
    let tree = source_tree(source);
    let parsed = tree.parsed_module().unwrap();
    let Expr::Lambda(params, body) = &parsed.program.functions[0].body else {
        panic!("expected lambda");
    };
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].mode, ParamMode::Read);
    assert_eq!(params[1].mode, ParamMode::Inout);
    assert_eq!(params[2].mode, ParamMode::Value);
    assert_eq!(**body, Expr::Var("z".into()));
    let path = AstPath::root(AstDeclaration::Function(0)).child(AstStep::FunctionBody);
    let map = tree.ast_source_map().unwrap();
    let parameter = path.child(AstStep::LambdaParameter(0));
    assert!(matches!(
        parameter.resolve(parsed),
        Some(AstNode::Parameter(_))
    ));
    assert_eq!(
        &source[map.origin(&parameter).unwrap().span.byte_range()],
        "read x: Option<Option<i32>>"
    );
    assert_eq!(
        &source[map
            .origin(&path.child(AstStep::LambdaBody))
            .unwrap()
            .span
            .byte_range()],
        "z"
    );
}

#[test]
fn postfix_calls_accept_expression_callees_and_keep_direct_named_calls() {
    assert!(matches!(expression("f(1)"), Expr::Call(_, _)));
    for source in [
        "f(1)(2)",
        "(if true { f } else { g })(1)",
        "fn(x) { x }(1)",
        "(fn(x) { x })(1)",
        "(f)(inout x)",
        "{ f }(1)",
    ] {
        assert!(matches!(expression(source), Expr::Apply(_, _)), "{source}");
        source_tree(&format!("fn main() {{ {source} }}"));
    }
    let Expr::Apply(callee, args) = expression("f(1)(2)") else {
        unreachable!()
    };
    assert_eq!(*callee, Expr::Call("f".into(), vec![Expr::Int(1)]));
    assert_eq!(args, [Expr::Int(2)]);
}

#[test]
fn whitespace_application_collects_arguments_without_changing_subtraction() {
    assert_eq!(
        expression("f x y"),
        Expr::Call(
            "f".into(),
            vec![Expr::Var("x".into()), Expr::Var("y".into())]
        )
    );
    assert_eq!(expression("f x (y)"), expression("f x y"));
    assert_eq!(
        expression("f x(y)"),
        Expr::Call(
            "f".into(),
            vec![Expr::Call("x".into(), vec![Expr::Var("y".into())])]
        )
    );
    assert_eq!(
        expression("f -1"),
        Expr::Binary(
            BinaryOp::Sub,
            Box::new(Expr::Var("f".into())),
            Box::new(Expr::Int(1))
        )
    );
    assert!(matches!(expression("f (-1)"), Expr::Call(_, _)));
    for source in [
        "f x y",
        "f x (y)",
        "f x(y)(z)",
        "(f) x",
        "m.f x",
        "f fn(x){x}",
        "if f x { 1 } else { 0 }",
        "while f x { () }",
    ] {
        source_tree(&format!("fn main() {{ {source} }}"));
    }
    parser::parse("let earlier = 1\nfn main() { 2 }").unwrap();
}

#[test]
fn operators_preserve_explicit_operand_order_and_precedence() {
    let Expr::FunctionOp(FunctionOp::PipeForward, left, right) = expression("1 + 2 |> f >> g")
    else {
        panic!("expected pipeline")
    };
    assert!(matches!(*left, Expr::Binary(BinaryOp::Add, _, _)));
    assert!(matches!(
        *right,
        Expr::FunctionOp(FunctionOp::ComposeForward, _, _)
    ));
    let Expr::FunctionOp(FunctionOp::PipeBackward, _, right) = expression("f <| g <| x") else {
        panic!("expected backward application")
    };
    assert!(matches!(
        *right,
        Expr::FunctionOp(FunctionOp::PipeBackward, _, _)
    ));
    let Expr::FunctionOp(FunctionOp::PipeForward, left, _) = expression("x |> f |> g") else {
        panic!("expected pipeline")
    };
    assert!(matches!(
        *left,
        Expr::FunctionOp(FunctionOp::PipeForward, _, _)
    ));
    assert!(matches!(
        expression("f << g"),
        Expr::FunctionOp(FunctionOp::ComposeBackward, _, _)
    ));
    for source in [
        "x |> f >> g",
        "f <| g <| x",
        "f << g",
        "f >> g == h",
        "x || y |> f",
    ] {
        let program = format!("fn main() {{ {source} }}");
        let tree = source_tree(&program);
        assert!(tree
            .nodes()
            .any(|(_, node)| node.kind() == SyntaxKind::FunctionOperatorExpression));
    }
}

#[test]
fn formatter_preserves_composition_nested_types_and_lambda_call_origins() {
    for source in [
        "fn main(){let f=fn(x){x};f 1 |> f >> f}",
        "fn main(){fn(x: Option<Option<i32>>){x}(None)}",
        "fn main(){(fn(x){x})(1)(2)}",
        "fn main(){f x(y)(z) (1) fn(x){x}(2)}",
        "fn main(){f<<g // keep composition\n>>h}",
        "fn main(){ f > > g }",
    ] {
        if source.contains("> >") {
            assert!(parser::parse(source).is_err());
            continue;
        }
        source_tree(source);
        let formatted = formatter::format(source).unwrap();
        assert_eq!(
            parser::parse_module(source),
            parser::parse_module(&formatted)
        );
        assert_eq!(formatter::format(&formatted).unwrap(), formatted);
        source_tree(&formatted);
    }
}

#[test]
fn malformed_lambdas_and_unbounded_application_chains_are_rejected() {
    for source in [
        "fn(x) => x",
        "fn x { x }",
        "fn(x) x",
        "fn(x:){x}",
        "f not x",
    ] {
        assert!(
            parser::parse(&format!("fn main(){{{source}}}")).is_err(),
            "{source}"
        );
    }
    for source in [
        format!("f{}", "(1)".repeat(70)),
        format!("f {}", "1 ".repeat(70)),
        format!("{}x", "f <| ".repeat(70)),
    ] {
        let error = parser::parse(&format!("fn main(){{{source}}}")).unwrap_err();
        assert!(error.contains("E-PARSE-DEPTH"), "{error}");
    }
}
