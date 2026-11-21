//! Lossless grammar trees recorded by the existing recursive-descent parser.
//! Recovery is tooling-only: erroneous files expose neither an AST nor a map.
use crate::{
    parser::ParsedModule,
    source::{LexemeKind, SourceFile, Span},
};
use std::ops::Range;

/// Deterministic within one immutable tree. Not stable across edits or trees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxKind {
    Module,
    Import,
    NativeDirective,
    Function,
    ExternFunction,
    ExternConstant,
    TypeDeclaration,
    Parameters,
    Parameter,
    TypeParameters,
    Variant,
    RecordField,
    TypeExpression,
    ForeignType,
    Name,
    Block,
    LocalBinding,
    ExpressionStatement,
    Expression,
    WhileExpression,
    MatchExpression,
    MatchArm,
    LetExpression,
    IfExpression,
    AssignmentExpression,
    BinaryExpression,
    UnaryExpression,
    ParenthesizedExpression,
    UnitExpression,
    CallExpression,
    ApplyExpression,
    LambdaExpression,
    FunctionOperatorExpression,
    Argument,
    FieldExpression,
    NameExpression,
    RecordExpression,
    MoveExpression,
    LiteralExpression,
    Pattern,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxElement {
    Node(NodeId),
    Lexeme(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxNode {
    kind: SyntaxKind,
    span: Span,
    children: Vec<SyntaxElement>,
}
impl SyntaxNode {
    pub fn kind(&self) -> SyntaxKind {
        self.kind
    }
    pub fn span(&self) -> Span {
        self.span
    }
    /// Ordered children partition this node, including internal trivia.
    pub fn children(&self) -> &[SyntaxElement] {
        &self.children
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticCode {
    Lexical,
    Syntax,
    Depth,
    ForeignType,
    NativeDirective,
    RecoveryLimit,
    MissingBindingSemicolon,
}
impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingBindingSemicolon => "E-PARSE-BINDING-SEMICOLON",
            Self::Lexical => "E-LEX",
            Self::Syntax => "E-PARSE",
            Self::Depth => "E-PARSE-DEPTH",
            Self::ForeignType => "E-FFI",
            Self::NativeDirective => "E-NATIVE-DIRECTIVE",
            Self::RecoveryLimit => "E-PARSE-RECOVERY-LIMIT",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub span: Span,
    pub message: String,
    pub insertion: Option<Insertion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insertion {
    pub position: crate::source::Position,
    pub text: &'static str,
    pub title: &'static str,
}
/// Vector index in this tree's unmodified ParsedModule, not a resolved symbol ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AstDeclaration {
    Function(usize),
    ExternFunction(usize),
    Constant(usize),
    Type(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclarationSource {
    pub declaration: AstDeclaration,
    pub node: NodeId,
}
#[derive(Clone, Debug)]
pub struct SyntaxTree<'s> {
    source: SourceFile<'s>,
    nodes: Vec<SyntaxNode>,
    root: NodeId,
    diagnostics: Vec<Diagnostic>,
    module: Option<ParsedModule>,
    declarations: Vec<DeclarationSource>,
}
impl<'s> SyntaxTree<'s> {
    /// Module syntax only. No dependency loading or semantic checks. Lexical
    /// failure retains the whole unlexed file and does not resume scanning.
    pub fn parse(text: &'s str) -> Self {
        match crate::lexer::lex_diagnostic(text) {
            Ok(source) => crate::parser::source_tree(source),
            Err(error) => {
                let source = SourceFile::unlexed(text);
                let span = source.lexemes()[0].span();
                Self {
                    source,
                    nodes: vec![
                        SyntaxNode {
                            kind: SyntaxKind::Error,
                            span,
                            children: vec![SyntaxElement::Lexeme(0)],
                        },
                        SyntaxNode {
                            kind: SyntaxKind::Module,
                            span,
                            children: vec![
                                SyntaxElement::Node(NodeId(0)),
                                SyntaxElement::Lexeme(1),
                            ],
                        },
                    ],
                    root: NodeId(1),
                    diagnostics: vec![Diagnostic {
                        code: DiagnosticCode::Lexical,
                        span: error.span,
                        message: error.message,
                        insertion: None,
                    }],
                    module: None,
                    declarations: Vec::new(),
                }
            }
        }
    }
    pub fn source(&self) -> &SourceFile<'s> {
        &self.source
    }
    pub fn root(&self) -> NodeId {
        self.root
    }
    /// IDs must come from this tree, not another parse.
    pub fn node(&self, id: NodeId) -> Option<&SyntaxNode> {
        self.nodes.get(id.0)
    }
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &SyntaxNode)> {
        self.nodes.iter().enumerate().map(|(i, n)| (NodeId(i), n))
    }
    pub fn node_text(&self, id: NodeId) -> Option<&'s str> {
        self.node(id)
            .map(|n| &self.source.text()[n.span.byte_range()])
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    /// Available only after a completely successful parse. Still requires the
    /// normal module resolution, type/ownership checks and native validation.
    pub fn parsed_module(&self) -> Option<&ParsedModule> {
        self.module.as_ref()
    }
    /// Empty after any diagnostic. Source order, including export/semicolons.
    pub fn declarations(&self) -> &[DeclarationSource] {
        &self.declarations
    }
    pub fn reconstruct(&self) -> String {
        let mut result = String::with_capacity(self.source.text().len());
        let mut work = vec![SyntaxElement::Node(self.root)];
        while let Some(element) = work.pop() {
            match element {
                SyntaxElement::Node(id) => {
                    work.extend(self.nodes[id.0].children.iter().rev().copied())
                }
                SyntaxElement::Lexeme(i) => result.push_str(self.source.lexemes()[i].text()),
            }
        }
        result
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Marker {
    start: usize,
    roots: usize,
    nodes: usize,
}
struct RawNode {
    kind: SyntaxKind,
    tokens: Range<usize>,
    children: Vec<NodeId>,
}
#[derive(Default)]
pub(crate) struct Builder {
    nodes: Vec<RawNode>,
    roots: Vec<NodeId>,
    diagnostics: Vec<(usize, DiagnosticCode, String)>,
    declarations: Vec<DeclarationSource>,
}
const MAX_SYNTAX_ERRORS: usize = 63;

impl Builder {
    pub fn mark(&self, start: usize) -> Marker {
        Marker {
            start,
            roots: self.roots.len(),
            nodes: self.nodes.len(),
        }
    }
    pub fn finish(&mut self, marker: Marker, end: usize, kind: SyntaxKind) -> Option<NodeId> {
        if marker.start == end && self.roots.len() == marker.roots {
            return None;
        }
        let children = self.roots.split_off(marker.roots);
        let id = NodeId(self.nodes.len());
        self.nodes.push(RawNode {
            kind,
            tokens: marker.start..end,
            children,
        });
        self.roots.push(id);
        Some(id)
    }
    // The parser initially reads the first child of `if let x = ...` as
    // a pattern. An `in` selects the legacy let expression, whose bare
    // binding name must retain the same CST role as any other let binder.
    pub fn finish_legacy_condition(&mut self, marker: Marker, end: usize) {
        let binder = self.roots[marker.roots];
        debug_assert_eq!(self.nodes[binder.0].kind, SyntaxKind::Pattern);
        self.nodes[binder.0].kind = SyntaxKind::Name;
        self.finish(marker, end, SyntaxKind::LetExpression);
    }
    /// Failed items become opaque before recovery can rewind a consumed prefix.
    pub fn rewind(&mut self, marker: Marker) {
        self.roots.truncate(marker.roots);
        self.nodes.truncate(marker.nodes);
    }
    pub fn map_declaration(&mut self, declaration: AstDeclaration, node: NodeId) {
        self.declarations
            .push(DeclarationSource { declaration, node });
    }
    pub fn diagnostic(&mut self, token: usize, code: DiagnosticCode, message: String) {
        self.diagnostics.push((token, code, message));
    }
    // Reserve the last ordinary diagnostic for an item that cannot recover.
    pub fn can_recover_binding(&self) -> bool {
        self.diagnostics.len() + 1 < MAX_SYNTAX_ERRORS
    }
    pub fn at_error_limit(&self) -> bool {
        self.diagnostics.len() >= MAX_SYNTAX_ERRORS
    }
    pub fn build(self, source: SourceFile<'_>, module: ParsedModule) -> SyntaxTree<'_> {
        let tokens: Vec<_> = source
            .lexemes()
            .iter()
            .enumerate()
            .filter_map(|(i, l)| matches!(l.kind(), LexemeKind::Token(_)).then_some(i))
            .collect();
        let mut ranges = Vec::new();
        let mut nodes = Vec::new();
        for raw in self.nodes {
            let start = tokens[raw.tokens.start];
            let end = if raw.tokens.is_empty() {
                start
            } else {
                tokens[raw.tokens.end - 1] + 1
            };
            let range = start..end;
            nodes.push(build_node(
                raw.kind,
                range.clone(),
                &raw.children,
                &ranges,
                &source,
            ));
            ranges.push(range);
        }
        let root = NodeId(nodes.len());
        nodes.push(build_node(
            SyntaxKind::Module,
            0..source.lexemes().len(),
            &self.roots,
            &ranges,
            &source,
        ));
        let diagnostics: Vec<_> = self
            .diagnostics
            .into_iter()
            .map(|(token, code, message)| Diagnostic {
                code,
                span: source.lexemes()[tokens[token]].span(),
                insertion: (code == DiagnosticCode::MissingBindingSemicolon).then(|| Insertion {
                    position: source.lexemes()[tokens[token - 1]].span().end,
                    text: ";",
                    title: "Insert ';' after local binding",
                }),
                message,
            })
            .collect();
        let (module, declarations) = if diagnostics.is_empty() {
            (Some(module), self.declarations)
        } else {
            (None, Vec::new())
        };
        SyntaxTree {
            source,
            nodes,
            root,
            diagnostics,
            module,
            declarations,
        }
    }
}
fn build_node(
    kind: SyntaxKind,
    range: Range<usize>,
    child_nodes: &[NodeId],
    ranges: &[Range<usize>],
    source: &SourceFile<'_>,
) -> SyntaxNode {
    let mut children = Vec::new();
    let mut cursor = range.start;
    for &child in child_nodes {
        let r = &ranges[child.0];
        debug_assert!(cursor <= r.start && r.end <= range.end);
        children.extend((cursor..r.start).map(SyntaxElement::Lexeme));
        children.push(SyntaxElement::Node(child));
        cursor = r.end;
    }
    children.extend((cursor..range.end).map(SyntaxElement::Lexeme));
    let start = source.lexemes()[range.start].span().start;
    let end = if range.is_empty() {
        start
    } else {
        source.lexemes()[range.end - 1].span().end
    };
    SyntaxNode {
        kind,
        span: Span { start, end },
        children,
    }
}
