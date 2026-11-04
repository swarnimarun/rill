use rill::{
    formatter, parser,
    source::{LexemeKind, Position},
    syntax::{AstDeclaration, DiagnosticCode, NodeId, SyntaxElement, SyntaxKind, SyntaxTree},
};
use std::{collections::HashSet, fs, path::Path};

fn check(source: &str) -> SyntaxTree<'_> {
    let tree = SyntaxTree::parse(source);
    assert_eq!(tree.reconstruct(), source);
    assert_eq!(tree.source().reconstruct(), source);
    assert_eq!(tree.node_text(tree.root()), Some(source));
    let mut work = vec![tree.root()];
    let mut seen = HashSet::new();
    let mut leaves = Vec::new();
    while let Some(id) = work.pop() {
        assert!(seen.insert(id));
        let node = tree.node(id).unwrap();
        let mut cursor = node.span().start;
        for child in node.children() {
            let span = match child {
                SyntaxElement::Node(child) => {
                    work.push(*child);
                    tree.node(*child).unwrap().span()
                }
                SyntaxElement::Lexeme(i) => {
                    leaves.push(*i);
                    tree.source().lexemes()[*i].span()
                }
            };
            assert_eq!(cursor, span.start, "{node:?}");
            assert!(span.start.byte <= span.end.byte);
            assert!(
                source.is_char_boundary(span.start.byte) && source.is_char_boundary(span.end.byte)
            );
            cursor = span.end;
        }
        assert_eq!(cursor, node.span().end);
    }
    assert_eq!(seen.len(), tree.nodes().count());
    leaves.sort_unstable();
    assert_eq!(
        leaves,
        (0..tree.source().lexemes().len()).collect::<Vec<_>>()
    );
    let again = SyntaxTree::parse(source);
    assert_eq!(
        tree.nodes().collect::<Vec<_>>(),
        again.nodes().collect::<Vec<_>>()
    );
    assert_eq!(tree.diagnostics(), again.diagnostics());
    assert_eq!(tree.declarations(), again.declarations());
    match parser::parse_module(source) {
        Ok(module) => {
            assert_eq!(tree.parsed_module(), Some(&module));
            assert!(tree.diagnostics().is_empty());
        }
        Err(error) => {
            assert!(tree.parsed_module().is_none());
            assert!(tree.declarations().is_empty());
            assert_eq!(tree.diagnostics()[0].message, error);
        }
    }
    tree
}
fn texts<'a>(tree: &'a SyntaxTree<'a>, kind: SyntaxKind) -> Vec<&'a str> {
    tree.nodes()
        .filter(|(_, n)| n.kind() == kind)
        .map(|(id, _)| tree.node_text(id).unwrap())
        .collect()
}
fn child_nodes(tree: &SyntaxTree<'_>, id: NodeId, kind: SyntaxKind) -> Vec<NodeId> {
    tree.node(id)
        .unwrap()
        .children()
        .iter()
        .filter_map(|e| match e {
            SyntaxElement::Node(n) if tree.node(*n).unwrap().kind() == kind => Some(*n),
            _ => None,
        })
        .collect()
}

#[test]
fn grammar_precedence_association_and_parentheses() {
    let tree = check("// λ🦀\r\nfn main(){ 1 + 2 * 3 - (4 + 5) // tail\r\n }");
    assert_eq!(
        texts(&tree, SyntaxKind::BinaryExpression),
        ["2 * 3", "1 + 2 * 3", "4 + 5", "1 + 2 * 3 - (4 + 5)"]
    );
    let ids: Vec<_> = tree
        .nodes()
        .filter(|(_, n)| n.kind() == SyntaxKind::BinaryExpression)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        child_nodes(&tree, ids[3], SyntaxKind::BinaryExpression),
        [ids[1]]
    );
    assert_eq!(
        child_nodes(&tree, ids[1], SyntaxKind::BinaryExpression),
        [ids[0]]
    );
    let unit = check("fn f(){(())}");
    assert_eq!(texts(&unit, SyntaxKind::UnitExpression), ["()"]);
    assert_eq!(texts(&unit, SyntaxKind::ParenthesizedExpression), ["(())"]);
}
#[test]
fn all_grammar_productions_are_real_nodes() {
    let src = r#"@include("missing.h"); import missing.module as m;
export type O<a> = None | Some(a)
type P = {x:i32; y:O<O<i32>>}
export extern const K:u32;
extern fn foreign(p:const_ptr<Thing>,n:i32)->unit;
export fn update(inout p:P,read n:i32){let mut x=-1; let q=record P{x=x;y=Some(None)}; p.x<-x+n; foreign(inout p.x,m.get(42)); while x<0{x<-x+1;}; if true{match Some(42){Some(v)=>v|None=>0}}else{0}}
let legacy (read x:i32) y=let z=move x in while false do ()"#;
    let tree = check(src);
    for kind in [
        SyntaxKind::Import,
        SyntaxKind::NativeDirective,
        SyntaxKind::Function,
        SyntaxKind::ExternFunction,
        SyntaxKind::ExternConstant,
        SyntaxKind::TypeDeclaration,
        SyntaxKind::Parameters,
        SyntaxKind::Parameter,
        SyntaxKind::TypeParameters,
        SyntaxKind::Variant,
        SyntaxKind::RecordField,
        SyntaxKind::TypeExpression,
        SyntaxKind::ForeignType,
        SyntaxKind::Name,
        SyntaxKind::Block,
        SyntaxKind::LocalBinding,
        SyntaxKind::ExpressionStatement,
        SyntaxKind::WhileExpression,
        SyntaxKind::MatchExpression,
        SyntaxKind::MatchArm,
        SyntaxKind::LetExpression,
        SyntaxKind::IfExpression,
        SyntaxKind::AssignmentExpression,
        SyntaxKind::BinaryExpression,
        SyntaxKind::UnaryExpression,
        SyntaxKind::UnitExpression,
        SyntaxKind::CallExpression,
        SyntaxKind::Argument,
        SyntaxKind::FieldExpression,
        SyntaxKind::NameExpression,
        SyntaxKind::RecordExpression,
        SyntaxKind::MoveExpression,
        SyntaxKind::LiteralExpression,
        SyntaxKind::Pattern,
    ] {
        assert!(!texts(&tree, kind).is_empty(), "{kind:?}");
    }
    assert!(texts(&tree, SyntaxKind::LocalBinding).contains(&"let mut x=-1;"));
    assert!(texts(&tree, SyntaxKind::Argument).contains(&"inout p.x"));
    assert!(texts(&tree, SyntaxKind::Parameter).contains(&"(read x:i32)"));
}
#[test]
fn declaration_mapping_distinguishes_duplicate_names() {
    let tree=check("export fn same(){1}; type Same=Same; export extern const SAME:u32; extern fn other()->unit; let same=2");
    for (m, (decl, text)) in tree.declarations().iter().zip([
        (AstDeclaration::Function(0), "export fn same(){1};"),
        (AstDeclaration::Type(0), "type Same=Same;"),
        (AstDeclaration::Constant(0), "export extern const SAME:u32;"),
        (
            AstDeclaration::ExternFunction(0),
            "extern fn other()->unit;",
        ),
        (AstDeclaration::Function(1), "let same=2"),
    ]) {
        assert_eq!(m.declaration, decl);
        assert_eq!(tree.node_text(m.node), Some(text));
    }
    assert_eq!(tree.declarations().len(), 5);
}
#[test]
fn exact_unicode_empty_comment_and_eof_spans() {
    for src in ["", "\t\r\n\u{2003}", "// 🦀 eof", "fn f(){\"é🦀\";()}"] {
        check(src);
    }
    let tree = check("// 🦀\r\n\tfn main(){\"é🦀\"}");
    let span = tree
        .nodes()
        .find(|(_, n)| n.kind() == SyntaxKind::LiteralExpression)
        .unwrap()
        .1
        .span();
    assert_eq!(
        span.start,
        Position {
            byte: 20,
            line: 2,
            column: 12
        }
    );
    assert_eq!(
        span.end,
        Position {
            byte: 28,
            line: 2,
            column: 16
        }
    );
}
#[test]
fn malformed_items_recover_without_ast_or_compilation() {
    let src="fn broken(){1 +}\nfn good(){42}\ntype Bad=A(\nexport fn later(){7}\nextern fn nope()->i64;\nfn last(){9}";
    let tree = check(src);
    assert_eq!(tree.diagnostics().len(), 3);
    assert_eq!(
        texts(&tree, SyntaxKind::Function),
        ["fn good(){42}", "export fn later(){7}", "fn last(){9}"]
    );
    assert_eq!(tree.diagnostics()[2].code, DiagnosticCode::ForeignType);
    assert!(formatter::format(src).is_err());
    assert!(rill::compile(src).is_err());
    for src in [
        "fn bad(){\nlet local=;\nfn good(){42}",
        "fn bad(){1+\nfn good(){42}",
        "let bad=fn good(){42}",
        "fn bad(\nfn good(){42}",
        "fn bad(){let local=;let nested=1;} fn good(){42}",
    ] {
        let t = check(src);
        assert_eq!(texts(&t, SyntaxKind::Function), ["fn good(){42}"]);
    }
    let tree = check("fn bad(){ 1 + ;\nfn(x){x}\nfn good(){42}");
    assert_eq!(tree.diagnostics().len(), 1);
    assert_eq!(texts(&tree, SyntaxKind::Function), ["fn good(){42}"]);
}
#[test]
fn diagnostic_positions_and_lexical_failure_bytes() {
    for src in [
        "fn bad(){",
        "export",
        "fn bad(){2147483648}",
        "@unknown(\"x\");",
        "extern fn f()->i64;",
        "// λ\r\n\t! later",
        "fn f(){λ}",
        "\"bad\\q\"",
        "c\"\\0\"",
        "\"abc",
        "\"line\nnext\"",
        "9999999999999999999999",
        "\"ends\\",
    ] {
        let tree = check(src);
        let d = &tree.diagnostics()[0];
        assert!(d
            .message
            .starts_with(&format!("{}:{}:", d.span.start.line, d.span.start.column)));
        if d.code == DiagnosticCode::Lexical {
            assert_eq!(tree.diagnostics().len(), 1);
            assert!(matches!(
                tree.source().lexemes()[0].kind(),
                LexemeKind::UnlexedSource
            ));
        }
    }
}
#[test]
fn recovery_limit_and_failed_native_rollback() {
    let many = "fn bad(){1 +}\n".repeat(200) + "fn good(){42}";
    let tree = check(&many);
    assert_eq!(tree.diagnostics().len(), 64);
    assert_eq!(
        tree.diagnostics().last().unwrap().code,
        DiagnosticCode::RecoveryLimit
    );
    let tree =
        check("@link(\"bad\\nname\"); @include(\"valid.h\"); @link(\"good\"); fn main(){42}");
    assert_eq!(tree.diagnostics().len(), 1);
    assert_eq!(
        texts(&tree, SyntaxKind::NativeDirective),
        ["@include(\"valid.h\");", "@link(\"good\");"]
    );
}
fn corpus(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    for e in fs::read_dir(path).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            corpus(&p, files)
        } else if p.extension().is_some_and(|x| x == "rill") {
            files.push(p);
        }
    }
}
#[test]
fn repository_corpus_roundtrips_ast_and_formatting() {
    let mut files = Vec::new();
    for dir in ["examples", "stdlib", "tests/fixtures"] {
        corpus(&Path::new(env!("CARGO_MANIFEST_DIR")).join(dir), &mut files);
    }
    assert!(files.len() >= 38);
    for path in files {
        let src = fs::read_to_string(path).unwrap();
        let tree = check(&src);
        assert!(tree.diagnostics().is_empty());
        let out = formatter::format(&src).unwrap();
        assert_eq!(tree.parsed_module(), check(&out).parsed_module());
        assert_eq!(formatter::format(&out).unwrap(), out);
    }
}
#[test]
fn deterministic_mutations_and_random_sequences() {
    let src="export type T<a> =None|Some(a)\nfn f(read x:T<i32>){let z=1;match x{Some(v)=>v+z|None=>0}}\nfn main(){f(Some(42))}";
    let lex = rill::source::SourceFile::lex(src).unwrap();
    for l in lex.lexemes() {
        if !matches!(l.kind(), LexemeKind::Token(_)) || l.text().is_empty() {
            continue;
        }
        for replacement in ["", "fn", "}", ";", "(", "\"🦀\""] {
            let r = l.span().byte_range();
            check(&format!(
                "{}{}{}",
                &src[..r.start],
                replacement,
                &src[r.end..]
            ));
        }
    }
    let tokens = [
        "fn",
        "let",
        "type",
        "export",
        "extern",
        "in",
        "if",
        "then",
        "else",
        "match",
        "record",
        "x",
        "A",
        "0",
        "\"é🦀\"",
        "{",
        "}",
        "(",
        ")",
        "<",
        ">",
        "=",
        "=>",
        "+",
        "|",
        ":",
        ";",
        ",",
        ".",
        "@",
    ];
    let mut state = 0x5eed_u64;
    for case in 0..1000 {
        let mut src = String::new();
        for _ in 0..case % 64 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            src.push_str(tokens[(state >> 32) as usize % tokens.len()]);
            src.push(if state & 3 == 0 { '\n' } else { ' ' });
        }
        check(&src);
    }
}
