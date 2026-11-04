use rill::{
    ast::*,
    parser,
    source_map::{AstNode, AstPath, AstSourceMap, AstStep as S, OriginKind},
    syntax::{AstDeclaration as D, SyntaxTree},
};
use std::{collections::BTreeSet, fs, path::Path};

fn expr_count(e: &Expr) -> usize {
    1 + match e {
        Expr::Let(_, a, b)
        | Expr::LetMut(_, a, b)
        | Expr::Binary(_, a, b)
        | Expr::FunctionOp(_, a, b)
        | Expr::While(a, b) => expr_count(a) + expr_count(b),
        Expr::If(c, a, b) => expr_count(c) + expr_count(a) + expr_count(b),
        Expr::Assign(_, v) | Expr::FieldAssign(_, _, v) => expr_count(v),
        Expr::Call(_, args) => args.iter().map(expr_count).sum(),
        Expr::Apply(callee, args) => {
            expr_count(callee) + args.iter().map(expr_count).sum::<usize>()
        }
        Expr::Lambda(params, body) => {
            expr_count(body)
                + params
                    .iter()
                    .map(|p| 1 + p.annotation.as_ref().map_or(0, type_count))
                    .sum::<usize>()
        }
        Expr::Record(_, fields) => fields.iter().map(|(_, v)| expr_count(v)).sum(),
        Expr::Match(v, arms) => {
            expr_count(v)
                + arms
                    .iter()
                    .map(|a| 1 + pattern_count(&a.pattern) + expr_count(&a.body))
                    .sum::<usize>()
        }
        _ => 0,
    }
}
fn pattern_count(p: &Pattern) -> usize {
    1 + match p {
        Pattern::Constructor(_, fields) => fields.iter().map(pattern_count).sum(),
        _ => 0,
    }
}
fn type_count(t: &TypeExpr) -> usize {
    1 + t.args.iter().map(type_count).sum::<usize>()
}
fn count(program: &Program) -> usize {
    program
        .functions
        .iter()
        .map(|f| {
            1 + expr_count(&f.body)
                + f.result_annotation.as_ref().map_or(0, type_count)
                + f.params
                    .iter()
                    .map(|p| 1 + p.annotation.as_ref().map_or(0, type_count))
                    .sum::<usize>()
        })
        .sum::<usize>()
        + program
            .externs
            .iter()
            .map(|f| 2 + 2 * f.params.len())
            .sum::<usize>()
        + 2 * program.constants.len()
        + program
            .types
            .iter()
            .map(|t| {
                1 + t
                    .variants
                    .iter()
                    .map(|v| 1 + v.fields.iter().map(type_count).sum::<usize>())
                    .sum::<usize>()
            })
            .sum::<usize>()
}
fn mapped(source: &str) -> (SyntaxTree<'_>, AstSourceMap) {
    let tree = SyntaxTree::parse(source);
    let before = parser::parse_module(source).unwrap();
    assert_eq!(tree.parsed_module(), Some(&before));
    let map = tree
        .ast_source_map()
        .unwrap_or_else(|e| panic!("{e:?}\n{source}"));
    assert_eq!(tree.parsed_module(), Some(&before));
    assert_eq!(parser::parse_module(source).unwrap(), before);
    assert_eq!(map.entries().len(), count(&before.program));
    assert_eq!(tree.ast_source_map().unwrap(), map);
    let mut paths = BTreeSet::new();
    for origin in map.entries() {
        assert!(paths.insert(origin.path.clone()));
        assert_eq!(origin.path.resolve(&before).unwrap().kind(), origin.kind);
        assert_eq!(
            origin.path.resolve_program(&before.program),
            origin.path.resolve(&before)
        );
        assert_eq!(map.origin(&origin.path), Some(origin));
        let node = tree.node(origin.syntax).unwrap();
        assert!(
            node.span().start.byte <= origin.span.start.byte
                && origin.span.end.byte <= node.span().end.byte
        );
        assert!(
            source.is_char_boundary(origin.span.start.byte)
                && source.is_char_boundary(origin.span.end.byte)
        );
    }
    (tree, map)
}
fn path(root: D, steps: &[S]) -> AstPath {
    AstPath {
        declaration: root,
        steps: steps.to_vec(),
    }
}
fn text<'a>(source: &'a str, map: &AstSourceMap, path: AstPath, kind: OriginKind) -> &'a str {
    let origin = map.origin(&path).unwrap_or_else(|| panic!("{path:?}"));
    assert_eq!(origin.provenance, kind);
    &source[origin.span.byte_range()]
}

#[test]
fn explicit_block_desugaring_and_unary_zero_have_real_origins() {
    let src = "fn main(){let x=1; -x; ()}";
    let (_, map) = mapped(src);
    let root = D::Function(0);
    assert_eq!(
        text(
            src,
            &map,
            path(root, &[S::FunctionBody]),
            OriginKind::BlockBinding
        ),
        "let x=1;"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(root, &[S::FunctionBody, S::LetValue]),
            OriginKind::Source
        ),
        "1"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(root, &[S::FunctionBody, S::LetBody]),
            OriginKind::BlockSequence
        ),
        "-x;"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(root, &[S::FunctionBody, S::LetBody, S::LetValue]),
            OriginKind::StatementDiscard
        ),
        "-x;"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(
                root,
                &[S::FunctionBody, S::LetBody, S::LetValue, S::LetValue]
            ),
            OriginKind::UnaryNegation
        ),
        "-x"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(
                root,
                &[
                    S::FunctionBody,
                    S::LetBody,
                    S::LetValue,
                    S::LetValue,
                    S::BinaryLeft
                ]
            ),
            OriginKind::UnaryNegation
        ),
        "-"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(
                root,
                &[S::FunctionBody, S::LetBody, S::LetValue, S::LetBody]
            ),
            OriginKind::ImplicitUnit
        ),
        ";"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(root, &[S::FunctionBody, S::LetBody, S::LetBody]),
            OriginKind::Source
        ),
        "()"
    );
}
#[test]
fn empty_and_fallthrough_units_are_distinct_from_written_unit() {
    for src in [
        "fn f(){}",
        "fn f(){1;}",
        "fn f(){let x=1;}",
        "fn f(){{};}",
        "fn f(){let mut x=1; x<-2;}",
    ] {
        let (_, map) = mapped(src);
        let unit = map.entries().last().unwrap();
        assert_eq!(unit.provenance, OriginKind::ImplicitUnit);
        assert_eq!(unit.span.start, unit.span.end);
        assert_eq!(&src[unit.span.start.byte..], "}");
    }
    let src = "fn f(){ (()) }";
    let (_, map) = mapped(src);
    assert_eq!(
        text(
            src,
            &map,
            path(D::Function(0), &[S::FunctionBody]),
            OriginKind::Source
        ),
        "()"
    );
}
#[test]
fn params_types_foreign_slots_record_variant_and_nested_patterns() {
    let src = r#"import other as o;
export type O<a> =None|Some(a)
type P={x:o.O<O<i32>>;y:i32}
extern const K:u32;
extern fn f(p:const_ptr<Thing>,n:i32)->ptr<Other>;
fn test(read p:P,inout q:O<i32>,x){match Some(Some(x)){(Some((Some(v))))=>v|None=>0}}
let legacy (read x:O<i32>) y=if true then x else y"#;
    let (tree, map) = mapped(src);
    let module = tree.parsed_module().unwrap();
    assert!(map
        .entries()
        .iter()
        .any(|e| e.provenance == OriginKind::RecordVariant));
    let p = path(
        D::Type(1),
        &[
            S::Variant(0),
            S::FieldType(0),
            S::TypeArgument(0),
            S::TypeArgument(0),
        ],
    );
    assert_eq!(text(src, &map, p.clone(), OriginKind::Source), "i32");
    assert!(matches!(p.resolve(module),Some(AstNode::TypeExpression(t))if t.name=="i32"));
    assert_eq!(
        text(
            src,
            &map,
            path(
                D::ExternFunction(0),
                &[S::ForeignParameter(0), S::ForeignParameterType]
            ),
            OriginKind::Source
        ),
        "const_ptr<Thing>"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(D::ExternFunction(0), &[S::ForeignResult]),
            OriginKind::Source
        ),
        "ptr<Other>"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(D::Constant(0), &[S::ConstantType]),
            OriginKind::Source
        ),
        "u32"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(
                D::Function(0),
                &[
                    S::FunctionBody,
                    S::Arm(0),
                    S::ArmPattern,
                    S::PatternField(0),
                    S::PatternField(0)
                ]
            ),
            OriginKind::Source
        ),
        "v"
    );
    assert_eq!(
        text(
            src,
            &map,
            path(D::Function(1), &[S::Parameter(0)]),
            OriginKind::Source
        ),
        "(read x:O<i32>)"
    );
}
#[test]
fn all_expression_and_pattern_families_map_without_changing_ast() {
    for src in [
        "fn f(){let mut x=0; while x<2{x<-x+1;}; if true{x}else{0}}",
        "fn f(){let p=record P{x=1;y=2};p.x<-3; g(inout p.x, inout p, p.x, move p, \"é🦀\",c\"ok\")}",
        "let f=let mut x=0 in let y=-x in if true then (1+2)*3 else while false do ()",
        "let f=match 0{0=>0|-1=>1|true=>2|false=>3|()=>4|_=>5|binding=>binding|Some(None,x)=>x}",
        "fn f(){-2147483648}","fn f(){--42}","fn f(){{{42}}}","fn f(){0-(1)}",
    ] {mapped(src);}
}
#[test]
fn validated_paths_reject_wrong_roles_missing_slots_and_out_of_range_indices() {
    let (tree, map) = mapped("type O<a> =None|Some(a) extern fn f(x:i32)->unit; fn main(x){x+1}");
    let module = tree.parsed_module().unwrap();
    for p in [
        path(D::Function(99), &[]),
        path(D::Function(0), &[S::ForeignResult]),
        path(D::Function(0), &[S::Parameter(1)]),
        path(D::Function(0), &[S::Parameter(0), S::Annotation]),
        path(D::Function(0), &[S::FunctionBody, S::Argument(0)]),
        path(
            D::Function(0),
            &[S::FunctionBody, S::BinaryLeft, S::FunctionBody],
        ),
        path(D::Type(0), &[S::Variant(0), S::FieldType(0)]),
        path(D::ExternFunction(0), &[S::ForeignParameter(5)]),
        path(D::Constant(0), &[S::ConstantType]),
    ] {
        assert!(p.resolve(module).is_none(), "{p:?}");
        assert!(map.origin(&p).is_none());
    }
}
#[test]
fn errors_never_expose_a_partial_map_and_order_is_deterministic() {
    for src in ["fn bad(){1 +}\nfn good(){42}", "fn f(){λ}", "fn f(){"] {
        let tree = SyntaxTree::parse(src);
        let error = tree.ast_source_map().unwrap_err();
        assert_eq!(error, tree.ast_source_map().unwrap_err());
        assert!(error.path.is_none());
        assert!(tree.parsed_module().is_none());
        assert!(parser::parse_module(src).is_err());
    }
    let (_, map) = mapped("type T=A fn f(){1+2} extern const K:u32; fn g(){3}");
    let roots: Vec<_> = map
        .entries()
        .iter()
        .filter(|e| e.path.steps.is_empty())
        .map(|e| e.path.declaration)
        .collect();
    assert_eq!(
        roots,
        [D::Type(0), D::Function(0), D::Constant(0), D::Function(1)]
    );
}
fn corpus(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    for e in fs::read_dir(path).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            corpus(&p, files)
        } else if p.extension().is_some_and(|x| x == "rill") {
            files.push(p)
        }
    }
}
#[test]
fn complete_corpus_has_one_origin_for_every_structural_ast_node() {
    let mut files = Vec::new();
    for dir in ["examples", "stdlib", "tests/fixtures"] {
        corpus(&Path::new(env!("CARGO_MANIFEST_DIR")).join(dir), &mut files);
    }
    assert!(files.len() >= 38);
    let mut total = 0;
    let mut kinds = BTreeSet::new();
    let mut origins = BTreeSet::new();
    for file in files {
        let src = fs::read_to_string(&file).unwrap();
        let (_, map) = mapped(&src);
        total += map.entries().len();
        for e in map.entries() {
            kinds.insert(format!("{:?}", e.kind));
            origins.insert(format!("{:?}", e.provenance));
        }
    }
    assert!(total > 1000);
    assert_eq!(kinds.len(), 12);
    assert!(origins.contains("Source"));
}
#[test]
fn accepted_mutations_also_have_total_maps() {
    let src="type T<a> =None|Some(a) fn f(read x:T<i32>){let z=1;match x{Some(v)=>v+z|None=>0}} fn main(){f(Some(42))}";
    let lex = rill::source::SourceFile::lex(src).unwrap();
    let mut accepted = 0;
    for l in lex.lexemes() {
        if l.text().is_empty() {
            continue;
        }
        for replacement in ["", "1", "x", "()", "(0)", ";", " "] {
            let r = l.span().byte_range();
            let s = format!("{}{}{}", &src[..r.start], replacement, &src[r.end..]);
            if parser::parse_module(&s).is_ok() {
                mapped(&s);
                accepted += 1;
            }
        }
    }
    assert!(accepted > 20);
}
#[test]
fn mapping_and_lookup_run_on_a_constrained_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for depth in [1, 8, 16, 24] {
                let src = format!("fn f(){{{}1{}}}", "(".repeat(depth), ")".repeat(depth));
                if parser::parse_module(&src).is_ok() {
                    mapped(&src);
                }
            }
            let src = format!("fn f(){{{}0}}", "let x=0;".repeat(20));
            mapped(&src);
            mapped(&format!("fn f(){{0{}}}", "+1".repeat(60)));
            mapped(&format!(
                "type T=C({}i32{})",
                "Box<".repeat(40),
                ">".repeat(40)
            ));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn empty_metadata_only_and_all_provenance_kinds_are_explicit() {
    for src in [
        "",
        "// only",
        "import absent as a;",
        "@include(\"missing.h\");",
    ] {
        let (_, map) = mapped(src);
        assert!(map.entries().is_empty());
    }
    let (_, map) = mapped("type P={x:i32} fn main(){let x=0; -x; not true;}");
    let kinds: BTreeSet<_> = map
        .entries()
        .iter()
        .map(|e| format!("{:?}", e.provenance))
        .collect();
    assert_eq!(
        kinds,
        [
            "Source",
            "BlockBinding",
            "BlockSequence",
            "StatementDiscard",
            "ImplicitUnit",
            "UnaryNegation",
            "BooleanNegation",
            "RecordVariant"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
}

#[test]
fn generated_accepted_expression_combinations_have_total_maps() {
    fn generate(state: &mut u64, depth: usize) -> String {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let choice = (*state >> 32) as usize;
        if depth == 0 {
            return [
                "0",
                "()",
                "x",
                "\"é🦀\"",
                "c\"ok\"",
                "true",
                "p.a",
                "move b",
                "-2147483648",
                "f(inout p.x)",
            ][choice % 10]
                .into();
        }
        let a = generate(state, depth - 1);
        let b = generate(state, depth - 1);
        match choice % 12 {
            0 => format!("({a})"),
            1 => format!("{{let x={a};{b}}}"),
            2 => format!("{{{a};{b}}}"),
            3 => format!("{{{a};}}"),
            4 => format!("if true {{{a}}} else {{{b}}}"),
            5 => format!("if true then {a} else {b}"),
            6 => format!("let x={a} in {b}"),
            7 => format!("while false do {a}"),
            8 => format!("({a})+({b})"),
            9 => format!("-({a})"),
            10 => format!("record P{{x={a};y={b}}}"),
            _ => format!("match {a}{{C(x)=>{b}|_=>0}}"),
        }
    }
    let mut state = 0x51de_u64;
    let mut accepted = 0;
    for _ in 0..1000 {
        let source = format!(
            "type T<a> = C(a)|D fn f(read p:T<i32>, x){{{}}}",
            generate(&mut state, 3)
        );
        if parser::parse_module(&source).is_ok() {
            mapped(&source);
            accepted += 1;
        }
    }
    assert!(accepted > 500, "only {accepted} generated cases parsed");
}

#[test]
fn closed_result_trees_have_complete_independent_counts_and_exact_utf8_spans() {
    let source =
        "// λ before result\nfn value(x: O<i32>) -> O<// é nested\ni32> {x}\nfn main() -> i32 {42}";
    let (_, map) = mapped(source);
    let result = path(D::Function(0), &[S::FunctionResult]);
    let origin = map.origin(&result).unwrap();
    assert_eq!(&source[origin.span.byte_range()], "O<// é nested\ni32>");
    let child = result.child(S::TypeArgument(0));
    assert_eq!(
        &source[map.origin(&child).unwrap().span.byte_range()],
        "i32"
    );
    for source in [
        "fn main() -> {42}",
        "fn main() -> O<i32 {42}",
        "fn main() -> O<> {42}",
    ] {
        let syntax = SyntaxTree::parse(source);
        assert_eq!(syntax.reconstruct(), source);
        assert!(syntax.parsed_module().is_none());
        assert!(syntax.ast_source_map().is_err());
    }
}
