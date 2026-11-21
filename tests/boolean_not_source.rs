use rill::{
    ast::Expr,
    formatter,
    modules::compile::{load_program, load_program_with_sources},
    source::{LexemeKind, SourceFile},
    source_map::{AstNode, AstPath, AstSourceMap, AstStep, OriginKind},
    syntax::{AstDeclaration, SyntaxKind, SyntaxTree},
};
use std::{collections::BTreeSet, fs};

fn mapped(source: &str) -> (SyntaxTree<'_>, AstSourceMap) {
    let tree = SyntaxTree::parse(source);
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    assert_eq!(tree.reconstruct(), source);
    let parsed = tree.parsed_module().unwrap();
    let map = tree.ast_source_map().unwrap();
    let mut work: Vec<_> = (0..parsed.program.functions.len())
        .map(|index| AstPath::root(AstDeclaration::Function(index)))
        .collect();
    let mut paths = BTreeSet::new();
    while let Some(path) = work.pop() {
        assert!(paths.insert(path.clone()));
        let node = path.resolve(parsed).unwrap();
        let origin = map.origin(&path).unwrap();
        assert_eq!(origin.kind, node.kind());
        let syntax_span = tree.node(origin.syntax).unwrap().span();
        assert!(syntax_span.start.byte <= origin.span.start.byte);
        assert!(origin.span.end.byte <= syntax_span.end.byte);
        work.extend(node.child_steps().into_iter().map(|step| path.child(step)));
    }
    assert_eq!(paths.len(), map.entries().len());
    (tree, map)
}

fn body() -> AstPath {
    AstPath::root(AstDeclaration::Function(0)).child(AstStep::FunctionBody)
}

fn origin_text<'a>(source: &'a str, map: &AstSourceMap, path: &AstPath) -> &'a str {
    &source[map.origin(path).unwrap().span.byte_range()]
}

#[test]
fn boolean_prefixes_have_unary_syntax_nodes() {
    for expression in ["not true", "!false"] {
        let source = format!("fn main() {{ {expression} }}");
        let (tree, _) = mapped(&source);
        let nodes: Vec<_> = tree
            .nodes()
            .filter(|(_, node)| node.kind() == SyntaxKind::UnaryExpression)
            .map(|(id, _)| tree.node_text(id).unwrap())
            .collect();
        assert_eq!(nodes, [expression]);
    }
}

#[test]
fn synthesized_false_is_anchored_to_the_boolean_operator() {
    for (operator, operand) in [("not", "true"), ("!", "false")] {
        let expression = format!("{operator} {operand}");
        let source = format!("// λ\nfn main() {{ {expression} }}");
        let (tree, map) = mapped(&source);
        let expression_path = body();
        let operand_path = expression_path.child(AstStep::BinaryLeft);
        let false_path = expression_path.child(AstStep::BinaryRight);
        assert_eq!(
            map.origin(&expression_path).unwrap().provenance,
            OriginKind::BooleanNegation
        );
        assert_eq!(origin_text(&source, &map, &expression_path), expression);
        assert_eq!(
            map.origin(&operand_path).unwrap().provenance,
            OriginKind::Source
        );
        assert_eq!(origin_text(&source, &map, &operand_path), operand);
        assert_eq!(
            map.origin(&false_path).unwrap().provenance,
            OriginKind::BooleanNegation
        );
        assert_eq!(origin_text(&source, &map, &false_path), operator);
        assert!(matches!(
            false_path.resolve(tree.parsed_module().unwrap()),
            Some(AstNode::Expression(Expr::Bool(false)))
        ));
    }
}

#[test]
fn nested_boolean_and_numeric_negation_keep_distinct_origins() {
    let source = "fn main() { not (!(-1 == -1)) }";
    let (_, map) = mapped(source);
    for kind in [OriginKind::BooleanNegation, OriginKind::UnaryNegation] {
        assert_eq!(
            map.entries()
                .iter()
                .filter(|origin| origin.provenance == kind)
                .count(),
            4
        );
    }
    let inner = body().child(AstStep::BinaryLeft);
    let comparison = inner.child(AstStep::BinaryLeft);
    assert_eq!(origin_text(source, &map, &inner), "!(-1 == -1)");
    assert_eq!(origin_text(source, &map, &comparison), "-1 == -1");
    assert_eq!(
        map.origin(&comparison).unwrap().provenance,
        OriginKind::Source
    );
    for step in [AstStep::BinaryLeft, AstStep::BinaryRight] {
        let negative = comparison.child(step);
        assert_eq!(origin_text(source, &map, &negative), "-1");
        assert_eq!(
            origin_text(source, &map, &negative.child(AstStep::BinaryLeft)),
            "-"
        );
    }
}

fn significant(source: &str) -> Vec<String> {
    SourceFile::lex(source)
        .unwrap()
        .lexemes()
        .iter()
        .filter(|piece| !matches!(piece.kind(), LexemeKind::Whitespace))
        .map(|piece| piece.text().to_owned())
        .collect()
}

#[test]
fn formatter_preserves_boolean_prefix_spellings_comments_and_parse() {
    for source in [
        "// not ! must stay\nfn main(){not(!false)&&!true}",
        "fn main(){! not // another operator\n false}",
    ] {
        let output = formatter::format(source).unwrap();
        assert_eq!(significant(source), significant(&output));
        assert_eq!(formatter::format(&output).unwrap(), output);
        assert_eq!(
            SyntaxTree::parse(source).parsed_module(),
            SyntaxTree::parse(&output).parsed_module()
        );
        mapped(&output);
    }
}

struct Directory(std::path::PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn module_loading_preserves_boolean_negation_origins() {
    let directory = Directory(
        std::env::temp_dir().join(format!("rill-boolean-not-source-{}", std::process::id())),
    );
    fs::create_dir(&directory.0).unwrap();
    fs::write(
        directory.0.join("helper.rill"),
        "export fn invert(value) { not value }",
    )
    .unwrap();
    let entry = directory.0.join("main.rill");
    fs::write(&entry, "import helper as h; fn main() { !h.invert(true) }").unwrap();
    let program = load_program_with_sources(&entry).unwrap();
    assert_eq!(program.program(), &load_program(&entry).unwrap());
    assert_eq!(program.sources().len(), 2);
    let mut operators = Vec::new();
    let mut negations = 0;
    for node in program.nodes() {
        for origin in &node.origins {
            if origin.provenance == OriginKind::BooleanNegation {
                negations += 1;
                if matches!(
                    program.node(&node.path),
                    Some(AstNode::Expression(Expr::Bool(false)))
                ) {
                    operators.push(program.origin_text(origin).unwrap());
                }
            }
        }
    }
    operators.sort_unstable();
    assert_eq!(operators, ["!", "not"]);
    assert_eq!(negations, 4);
}
