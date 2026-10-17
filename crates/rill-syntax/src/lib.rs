//! Lossless Concrete Syntax Tree, token kinds and the operator precedence
//! table (PLAN.md §2.1 / §2.5).
//!
//! The CST keeps *every* token — including trivia — and every token appears
//! exactly once, in source order. Concatenating the token texts of a CST
//! reproduces the original source byte-for-byte (verified by round-trip tests).

use std::fmt;

use rill_span::{SourceMap, Span};

// ---------------------------------------------------------------------------
// SyntaxKind
// ---------------------------------------------------------------------------

/// Kind of a token or of an interior CST node.
///
/// Token kinds come first (and are what the lexer produces); the remainder are
/// interior node kinds produced by the parser.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // ---- trivia -----------------------------------------------------------
    Whitespace,
    LineComment,
    BlockComment,

    // ---- literals ---------------------------------------------------------
    IntLiteral,
    FloatLiteral,
    Text,
    RawText,
    CharLiteral,

    // ---- identifiers ------------------------------------------------------
    Name,

    // ---- punctuation ------------------------------------------------------
    Semi,       // ;
    Comma,      // ,
    Colon,      // :
    ColonColon, // ::
    Dot,        // .
    DotDot,     // ..
    DotDotLt,   // ..<
    Arrow,      // ->
    Assign,     // =
    ColonEq,    // :=
    EqEq,       // ==
    BangEq,     // !=
    Lt,         // <
    Gt,         // >
    LtEq,       // <=
    GtEq,       // >=
    PlusPlus,   // ++
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Percent,    // %
    Shl,        // <<
    Shr,        // >>
    Amp,        // &
    PipeAmp,    // |&
    Pipe,       // |
    Caret,      // ^
    LParen,     // (
    RParen,     // )
    LBracket,   // [
    RBracket,   // ]
    LBrace,     // {
    RBrace,     // }
    Question,   // ?
    Bang,       // !
    Underscore, // _

    // ---- keywords ---------------------------------------------------------
    KwType,
    KwTrait,
    KwImpl,
    KwDyn,
    KwWhere,
    KwIs,
    KwMatch,
    KwRecord,
    KwView,
    KwWhen,
    KwIf,
    KwElse,
    KwTrue,
    KwFalse,
    KwAs,
    KwSize,
    KwShapeof,
    KwReshape,
    KwLet,
    KwUse,
    KwFor,
    KwVar,
    KwSet,
    KwInout,
    KwSink,
    KwProject,
    KwYield,
    KwComptime,
    KwBuilder,
    KwExtern,
    KwUnsafe,
    KwDeinit,
    KwAnd, // contextual binary `and`
    KwOr,  // contextual binary `or`
    KwNot, // contextual prefix `not`

    // ---- lexer artifacts --------------------------------------------------
    Error,
    Eof,

    // ---- interior node kinds ---------------------------------------------
    // Declarations & statements
    Root,
    UseDecl,
    UsePath,
    UseGroup,
    SignatureDecl,
    LetDecl,
    ProjectDecl,
    TypeDecl,
    TypeParams,
    TraitDecl,
    ImplDecl,
    BuilderDecl,
    ExternDecl,
    VarStmt,
    SetStmt,
    YieldStmt,
    BindStmt,
    ExprStmt,
    GuardStmt,
    TryBindStmt, // `let? x = expr;`
    WhenStmt,
    ForStmt,
    WhileStmt,
    BreakStmt,
    // Types
    TypeRef,
    TypePath,
    TypeApp,
    TypeTuple,
    TypeFn,
    TypeArray,
    TypePtr,
    TypeDyn,
    TypeView,
    RecordDef,
    FieldDef,
    DeinitClause,
    VariantList,
    Variant,
    TraitBody,
    ImplBody,
    BuilderBody,
    // Signatures
    ParamList,
    Param,
    ConstraintList,
    Constraint,
    EffectRow,
    EffectItem,
    // Expressions
    ParenExpr,
    TupleExpr,
    ArrayExpr,
    BlockExpr,
    LambdaExpr,
    LambdaParams,
    PathExpr,
    LiteralExpr,
    FieldExpr,
    CallExpr,
    IndexExpr, // `e[args]` — adjacent bracket application (index or failable call)
    TryExpr,   // e?
    RangeExpr,
    BinaryExpr,
    UnaryExpr,
    FlowExpr, // `.`
    IfExpr,
    MatchExpr,
    MatchArm,
    MatchPat,
    Block, // `{ stmts; expr? }`
    BuilderExpr,
    ConcurrentExpr,
    ComptimeExpr,
    UnsafeExpr,
    ArgList,
    Arg,
}

impl SyntaxKind {
    /// Trivia tokens are attached to the tree but skipped by the parser.
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment
        )
    }

    pub fn is_literal(self) -> bool {
        matches!(
            self,
            SyntaxKind::IntLiteral
                | SyntaxKind::FloatLiteral
                | SyntaxKind::Text
                | SyntaxKind::RawText
                | SyntaxKind::CharLiteral
                | SyntaxKind::KwTrue
                | SyntaxKind::KwFalse
        )
    }

    pub fn is_keyword(self) -> bool {
        use SyntaxKind::*;
        matches!(
            self,
            KwType
                | KwTrait
                | KwImpl
                | KwDyn
                | KwWhere
                | KwIs
                | KwMatch
                | KwRecord
                | KwView
                | KwWhen
                | KwIf
                | KwElse
                | KwTrue
                | KwFalse
                | KwAs
                | KwSize
                | KwShapeof
                | KwReshape
                | KwLet
                | KwUse
                | KwFor
                | KwVar
                | KwSet
                | KwInout
                | KwSink
                | KwProject
                | KwYield
                | KwComptime
                | KwBuilder
                | KwExtern
                | KwUnsafe
                | KwDeinit
        )
    }

    /// The fixed (reserved) keyword table from PLAN.md §2.1.
    ///
    /// `and`, `or`, `not` are *contextual*: they lex as [`SyntaxKind::Name`]
    /// like every other identifier and are only treated as operators by the
    /// [`Operator`] table below, so user code may still bind those names where
    /// an operator is not expected.
    pub fn from_keyword(text: &str) -> Option<SyntaxKind> {
        use SyntaxKind::*;
        Some(match text {
            "type" => KwType,
            "trait" => KwTrait,
            "impl" => KwImpl,
            "dyn" => KwDyn,
            "where" => KwWhere,
            "is" => KwIs,
            "match" => KwMatch,
            "record" => KwRecord,
            "view" => KwView,
            "when" => KwWhen,
            "if" => KwIf,
            "else" => KwElse,
            "true" => KwTrue,
            "false" => KwFalse,
            "as" => KwAs,
            "size" => KwSize,
            "shapeof" => KwShapeof,
            "reshape" => KwReshape,
            "let" => KwLet,
            "use" => KwUse,
            "for" => KwFor,
            "var" => KwVar,
            "set" => KwSet,
            "inout" => KwInout,
            "sink" => KwSink,
            "project" => KwProject,
            "yield" => KwYield,
            "comptime" => KwComptime,
            "builder" => KwBuilder,
            "extern" => KwExtern,
            "unsafe" => KwUnsafe,
            "deinit" => KwDeinit,
            _ => return None,
        })
    }

    pub fn keyword_text(self) -> Option<&'static str> {
        use SyntaxKind::*;
        Some(match self {
            KwType => "type",
            KwTrait => "trait",
            KwImpl => "impl",
            KwDyn => "dyn",
            KwWhere => "where",
            KwIs => "is",
            KwMatch => "match",
            KwRecord => "record",
            KwView => "view",
            KwWhen => "when",
            KwIf => "if",
            KwElse => "else",
            KwTrue => "true",
            KwFalse => "false",
            KwAs => "as",
            KwSize => "size",
            KwShapeof => "shapeof",
            KwReshape => "reshape",
            KwLet => "let",
            KwUse => "use",
            KwFor => "for",
            KwVar => "var",
            KwSet => "set",
            KwInout => "inout",
            KwSink => "sink",
            KwProject => "project",
            KwYield => "yield",
            KwComptime => "comptime",
            KwBuilder => "builder",
            KwExtern => "extern",
            KwUnsafe => "unsafe",
            KwDeinit => "deinit",
            _ => return None,
        })
    }

    /// The token's fixed source text, for fixed-spelling tokens (punctuation
    /// and reserved keywords). Literals/identifiers return `None`.
    pub fn token_text(self) -> Option<&'static str> {
        use SyntaxKind::*;
        if let Some(kw) = self.keyword_text() {
            return Some(kw);
        }
        Some(match self {
            Semi => ";",
            Comma => ",",
            Colon => ":",
            ColonColon => "::",
            Dot => ".",
            DotDot => "..",
            DotDotLt => "..<",
            Arrow => "->",
            Assign => "=",
            ColonEq => ":=",
            EqEq => "==",
            BangEq => "!=",
            Lt => "<",
            Gt => ">",
            LtEq => "<=",
            GtEq => ">=",
            PlusPlus => "++",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            Shl => "<<",
            Shr => ">>",
            Amp => "&",
            PipeAmp => "|&",
            Pipe => "|",
            Caret => "^",
            LParen => "(",
            RParen => ")",
            LBracket => "[",
            RBracket => "]",
            LBrace => "{",
            RBrace => "}",
            Question => "?",
            Bang => "!",
            Underscore => "_",
            Eof => "",
            _ => return None,
        })
    }

    pub fn is_path_segment(self) -> bool {
        self == SyntaxKind::Name || self == SyntaxKind::Underscore
    }

    /// Can this token begin a primary expression / application argument?
    pub fn starts_expr(self) -> bool {
        use SyntaxKind::*;
        matches!(
            self,
            Name | Underscore
                | IntLiteral
                | FloatLiteral
                | Text
                | RawText
                | CharLiteral
                | KwTrue
                | KwFalse
                | KwIf
                | KwMatch
                | KwComptime
                | KwUnsafe
                | KwSize
                | KwShapeof
                | KwReshape
                | LParen
                | LBracket
                | LBrace
        )
    }
}

impl fmt::Display for SyntaxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.token_text() {
            Some(t) => write!(f, "`{t}`"),
            None => write!(f, "{self:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// A lexed token. `span` is the token's byte range in the global position space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyntaxToken {
    pub kind: SyntaxKind,
    pub span: Span,
}

impl SyntaxToken {
    pub fn new(kind: SyntaxKind, span: Span) -> Self {
        SyntaxToken { kind, span }
    }

    /// Copy of the token's byte span.
    #[inline]
    pub fn span(self) -> Span {
        self.span
    }

    pub fn text(self, sm: &SourceMap) -> &str {
        sm.snippet(self.span)
    }

    pub fn is_trivia(self) -> bool {
        self.kind.is_trivia()
    }
}

// ---------------------------------------------------------------------------
// CST
// ---------------------------------------------------------------------------

/// A child of an interior node: either a token or a nested node.
///
/// Trivia tokens are ordinary children, which is what makes the tree lossless.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Element {
    Token(SyntaxToken),
    Node(SyntaxNode),
}

impl Element {
    pub fn span(&self) -> Span {
        match self {
            Element::Token(t) => t.span,
            Element::Node(n) => n.span,
        }
    }

    pub fn kind(&self) -> SyntaxKind {
        match self {
            Element::Token(t) => t.kind,
            Element::Node(n) => n.kind,
        }
    }

    pub fn as_node(&self) -> Option<&SyntaxNode> {
        match self {
            Element::Node(n) => Some(n),
            _ => None,
        }
    }
}

/// An interior CST node.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SyntaxNode {
    pub kind: SyntaxKind,
    /// Byte span covering all children (dummy for empty synthetic nodes).
    pub span: Span,
    pub children: Vec<Element>,
}

impl SyntaxNode {
    pub fn new(kind: SyntaxKind, children: Vec<Element>) -> Self {
        let span = children
            .iter()
            .map(|c| c.span())
            .fold(Span::DUMMY, |acc, s| acc.merge(s));
        SyntaxNode {
            kind,
            span,
            children,
        }
    }

    /// Iterates children, skipping trivia.
    pub fn children_no_trivia(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter(|c| !c.kind().is_trivia())
    }

    /// Child nodes only, skipping trivia.
    pub fn nodes(&self) -> impl Iterator<Item = &SyntaxNode> {
        self.children_no_trivia().filter_map(|c| c.as_node())
    }

    /// Non-trivia tokens directly inside this node.
    pub fn tokens(&self) -> impl Iterator<Item = &SyntaxToken> {
        self.children_no_trivia().filter_map(|c| match c {
            Element::Token(t) => Some(t),
            _ => None,
        })
    }

    /// First direct non-trivia child node of the given kind.
    pub fn child_node(&self, kind: SyntaxKind) -> Option<&SyntaxNode> {
        self.nodes().find(|n| n.kind == kind)
    }

    /// All direct non-trivia child nodes of the given kind.
    pub fn child_nodes(&self, kind: SyntaxKind) -> impl Iterator<Item = &SyntaxNode> {
        self.nodes().filter(move |n| n.kind == kind)
    }

    /// True when a direct non-trivia child token of `kind` exists.
    pub fn has_token(&self, kind: SyntaxKind) -> bool {
        self.tokens().any(|t| t.kind == kind)
    }

    /// First direct non-trivia child token of the given kind.
    pub fn child_token(&self, kind: SyntaxKind) -> Option<&SyntaxToken> {
        self.tokens().find(|t| t.kind == kind)
    }

    /// Reconstructs the source text of this subtree.
    pub fn text(&self, sm: &SourceMap) -> String {
        let mut out = String::new();
        self.write_text(sm, &mut out);
        out
    }

    fn write_text(&self, sm: &SourceMap, out: &mut String) {
        for child in &self.children {
            match child {
                Element::Token(t) => out.push_str(t.text(sm)),
                Element::Node(n) => n.write_text(sm, out),
            }
        }
    }

    /// Depth-first iteration over all tokens, trivia included, in source order.
    pub fn all_tokens(&self, out: &mut Vec<SyntaxToken>) {
        for child in &self.children {
            match child {
                Element::Token(t) => out.push(*t),
                Element::Node(n) => n.all_tokens(out),
            }
        }
    }

    /// Depth-first (pre-order) iteration over this node and its descendants.
    pub fn preorder(&self) -> Vec<&SyntaxNode> {
        let mut out = Vec::new();
        self.collect_preorder(&mut out);
        out
    }

    fn collect_preorder<'n>(&'n self, out: &mut Vec<&'n SyntaxNode>) {
        out.push(self);
        for child in self.nodes() {
            child.collect_preorder(out);
        }
    }

    /// All descendant nodes of the given kind, pre-order.
    pub fn descendants(&self, kind: SyntaxKind) -> Vec<&SyntaxNode> {
        self.preorder()
            .into_iter()
            .filter(|n| n.kind == kind)
            .collect()
    }

    /// First descendant (including `self`) of the given kind.
    pub fn find(&self, kind: SyntaxKind) -> Option<&SyntaxNode> {
        self.preorder().into_iter().find(|n| n.kind == kind)
    }

    /// Count of descendant nodes of the given kind.
    pub fn count(&self, kind: SyntaxKind) -> usize {
        self.descendants(kind).len()
    }

    /// Count of descendant *tokens* of the given kind. `descendants` is
    /// node-only (which is what lowering wants), so token assertions use this.
    pub fn count_tokens(&self, kind: SyntaxKind) -> usize {
        let mut tokens = Vec::new();
        self.all_tokens(&mut tokens);
        tokens.iter().filter(|t| t.kind == kind).count()
    }

    /// True when a descendant token of the given kind exists.
    pub fn has_descendant_token(&self, kind: SyntaxKind) -> bool {
        self.count_tokens(kind) > 0
    }

    /// Direct child nodes, in order (trivia skipped).
    pub fn child_list(&self) -> Vec<&SyntaxNode> {
        self.nodes().collect()
    }

    /// Pretty-prints the tree for tests and debugging.
    pub fn dump(&self, sm: &SourceMap) -> String {
        let mut out = String::new();
        self.dump_into(sm, 0, &mut out);
        out
    }

    fn dump_into(&self, sm: &SourceMap, indent: usize, out: &mut String) {
        use std::fmt::Write as _;
        let pad = "  ".repeat(indent);
        match self.kind.token_text() {
            Some(fixed) if self.children.is_empty() => {
                let _ = writeln!(out, "{pad}{}({fixed})", self.kind_str());
            }
            _ => {
                let _ = writeln!(out, "{pad}{}", self.kind_str());
                for child in &self.children {
                    match child {
                        Element::Token(t) => {
                            let _ = writeln!(out, "{pad}  {} {:?}", t.kind_str(), t.text(sm));
                        }
                        Element::Node(n) => n.dump_into(sm, indent + 1, out),
                    }
                }
            }
        }
    }

    fn kind_str(&self) -> String {
        format!("{:?}@{:?}", self.kind, self.span)
    }
}

/// Lossless concrete syntax tree for one file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cst {
    pub root: SyntaxNode,
}

impl Cst {
    pub fn new(root: SyntaxNode) -> Self {
        Cst { root }
    }

    /// Full reconstructed source text.
    pub fn text(&self, sm: &SourceMap) -> String {
        self.root.text(sm)
    }

    /// All tokens in source order (trivia included).
    pub fn tokens(&self) -> Vec<SyntaxToken> {
        let mut out = Vec::new();
        self.root.all_tokens(&mut out);
        out
    }

    /// True when the CST reproduces `expected` byte-for-byte.
    pub fn matches_source(&self, sm: &SourceMap, expected: &str) -> bool {
        self.text(sm) == expected
    }
}

impl SyntaxToken {
    fn kind_str(self) -> String {
        format!("{:?}", self.kind)
    }
}

// ---------------------------------------------------------------------------
// Operator precedence (PLAN.md §2.5)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Assoc {
    Left,
    Right,
    /// Chains without parentheses are an error.
    NonAssoc,
}

/// A binary (or postfix) operator, with its binding power.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Operator {
    pub kind: SyntaxKind,
    /// Higher binds tighter.
    pub bp: u8,
    pub assoc: Assoc,
    /// Human-readable spelling, for diagnostics.
    pub spelling: &'static str,
}

/// Binding powers, one tier per row of the §2.5 table.
///
/// The table is listed "lowest to highest": level 1 (`|`) is `BP_CHOICE`,
/// level 2 (ranges) is `BP_RANGE`, and so on.
///
/// Resolution note: the plan's level 12 (postfix: `f[x]`, `xs[i]`, `e?`,
/// `f(x)`) and level 13 (application `f x`) are *both* handled by the atom
/// parser rather than by the binary Pratt loop. Postfix forms bind to the
/// immediately preceding atom (this is what makes the adjacency rule
/// `xs[i]` vs `xs [i]` meaningful), which is stricter than a literal reading of
/// the table where application outranked postfix; with the literal reading the
/// adjacency rule would be dead syntax.
pub mod bp {
    pub const CHOICE: u8 = 1; // |
    pub const RANGE: u8 = 2; // .. ..<
    pub const OR: u8 = 3; // or
    pub const AND: u8 = 4; // and
    pub const EQUALITY: u8 = 5; // = == !=
    pub const COMPARE: u8 = 6; // < > <= >=
    pub const CONCAT: u8 = 7; // ++
    pub const ADDITIVE: u8 = 8; // + - ^ |&
    pub const MULTIPLICATIVE: u8 = 9; // * / % << >> &
    pub const CAST: u8 = 10; // as is
    /// `.` — uniform call / field projection.
    ///
    /// The §2.5 table places `.` *below* concatenation and arithmetic, which
    /// would read `from.balance - amount` as `from . (balance - amount)` and
    /// `alice.balance as String` as `alice . (balance as String)`. The table's
    /// own examples (`x . f y ≡ f x y`, `xs . split_at[i]`, `from.balance`)
    /// only work when `.` binds tighter than every other binary operator while
    /// staying looser than application, so that is what is implemented.
    pub const FLOW: u8 = 11;
}

/// Looks up the binary operator for `kind`, if it is one.
///
/// `and`, `or` and `not` are contextual: the parser only consults this after
/// recognizing a [`SyntaxKind::Name`] whose text matches.
pub fn binary_operator(kind: SyntaxKind) -> Option<Operator> {
    use SyntaxKind::*;
    let (bp, assoc, spelling) = match kind {
        Pipe => (bp::CHOICE, Assoc::Left, "|"),
        DotDot => (bp::RANGE, Assoc::NonAssoc, ".."),
        DotDotLt => (bp::RANGE, Assoc::NonAssoc, "..<"),
        KwOr => (bp::OR, Assoc::Left, "or"),
        KwAnd => (bp::AND, Assoc::Left, "and"),
        Assign => (bp::EQUALITY, Assoc::NonAssoc, "="),
        EqEq => (bp::EQUALITY, Assoc::NonAssoc, "=="),
        BangEq => (bp::EQUALITY, Assoc::NonAssoc, "!="),
        Lt => (bp::COMPARE, Assoc::NonAssoc, "<"),
        Gt => (bp::COMPARE, Assoc::NonAssoc, ">"),
        LtEq => (bp::COMPARE, Assoc::NonAssoc, "<="),
        GtEq => (bp::COMPARE, Assoc::NonAssoc, ">="),
        Dot => (bp::FLOW, Assoc::Left, "."),
        PlusPlus => (bp::CONCAT, Assoc::Right, "++"),
        Plus => (bp::ADDITIVE, Assoc::Left, "+"),
        Minus => (bp::ADDITIVE, Assoc::Left, "-"),
        Caret => (bp::ADDITIVE, Assoc::Left, "^"),
        PipeAmp => (bp::ADDITIVE, Assoc::Left, "|&"),
        Star => (bp::MULTIPLICATIVE, Assoc::Left, "*"),
        Slash => (bp::MULTIPLICATIVE, Assoc::Left, "/"),
        Percent => (bp::MULTIPLICATIVE, Assoc::Left, "%"),
        Shl => (bp::MULTIPLICATIVE, Assoc::Left, "<<"),
        Shr => (bp::MULTIPLICATIVE, Assoc::Left, ">>"),
        Amp => (bp::MULTIPLICATIVE, Assoc::Left, "&"),
        KwAs => (bp::CAST, Assoc::Left, "as"),
        KwIs => (bp::CAST, Assoc::Left, "is"),
        _ => return None,
    };
    Some(Operator {
        kind,
        bp,
        assoc,
        spelling,
    })
}

/// Contextual operator names recognized as operators only in operator
/// position. `a and b` parses as a binary expression; `and` alone remains a
/// legal identifier.
pub fn contextual_operator(text: &str) -> Option<SyntaxKind> {
    match text {
        "and" => Some(SyntaxKind::KwAnd),
        "or" => Some(SyntaxKind::KwOr),
        "not" => Some(SyntaxKind::KwNot),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_table_is_consistent() {
        for kw in ["type", "yield", "deinit", "unsafe", "builder", "comptime"] {
            let k = SyntaxKind::from_keyword(kw).unwrap();
            assert!(k.is_keyword());
            assert_eq!(k.keyword_text(), Some(kw));
            assert_eq!(k.token_text(), Some(kw));
        }
        assert_eq!(SyntaxKind::from_keyword("option"), None); // contextual
        assert_eq!(SyntaxKind::from_keyword("alloc"), None); // contextual
        assert_eq!(SyntaxKind::from_keyword("sequence"), None);
    }

    #[test]
    fn precedence_order_matches_plan() {
        assert!(
            binary_operator(SyntaxKind::Pipe).unwrap().bp
                < binary_operator(SyntaxKind::DotDot).unwrap().bp
        );
        // `.` binds tighter than every other binary operator (see `bp::FLOW`),
        // so `from.balance - amount` groups as `(from.balance) - amount` and
        // `alice.balance as String` casts the field access.
        assert!(
            binary_operator(SyntaxKind::Star).unwrap().bp
                < binary_operator(SyntaxKind::Dot).unwrap().bp
        );
        assert!(
            binary_operator(SyntaxKind::KwAs).unwrap().bp
                < binary_operator(SyntaxKind::Dot).unwrap().bp
        );
        assert!(
            binary_operator(SyntaxKind::PlusPlus).unwrap().bp
                < binary_operator(SyntaxKind::Plus).unwrap().bp
        );
        assert!(
            binary_operator(SyntaxKind::Star).unwrap().bp
                < binary_operator(SyntaxKind::KwAs).unwrap().bp
        );
        assert_eq!(
            binary_operator(SyntaxKind::PlusPlus).unwrap().assoc,
            Assoc::Right
        );
        assert_eq!(
            binary_operator(SyntaxKind::Assign).unwrap().assoc,
            Assoc::NonAssoc
        );
        assert_eq!(
            binary_operator(SyntaxKind::Slash).unwrap().assoc,
            Assoc::Left
        );
        assert!(binary_operator(SyntaxKind::Semi).is_none());
        assert!(binary_operator(SyntaxKind::DotDotLt).is_some());
    }

    #[test]
    fn node_span_covers_children() {
        let root = SyntaxNode::new(
            SyntaxKind::Root,
            vec![
                Element::Token(SyntaxToken::new(SyntaxKind::Name, Span::range(5, 8))),
                Element::Node(SyntaxNode::new(
                    SyntaxKind::Block,
                    vec![Element::Token(SyntaxToken::new(
                        SyntaxKind::LBrace,
                        Span::range(10, 11),
                    ))],
                )),
            ],
        );
        assert_eq!(root.span, Span::range(5, 11));
    }
}
