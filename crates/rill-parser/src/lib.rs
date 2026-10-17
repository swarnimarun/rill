//! Resilient Pratt + recursive-descent parser for Rill (PLAN.md §2, §7.1).
//!
//! The parser never rewrites the source: it emits an event stream
//! (`Start`/`Token`/`Finish`) that [`build`] replays into a lossless
//! [`Cst`]. Trivia tokens are never consumed by the parser; they are
//! interleaved at build time into whichever node is open when the next real
//! token is consumed, which guarantees that every token appears exactly once
//! and in source order.
//!
//! # Adjacency rule (§2.5, M0)
//!
//! Postfix forms attach by *adjacency*: a `[`, `(` or `?` with no intervening
//! trivia binds to the preceding atom.
//!
//! | Surface | CST |
//! |---|---|
//! | `xs[i]`, `f[x, y]` | [`SyntaxKind::IndexExpr`] with a bracketed arg list |
//! | `f(x, y)` | [`SyntaxKind::CallExpr`] with a parenthesized arg list |
//! | `f (x)` | application of `f` to one grouped argument |
//! | `f [x]` | application of `f` to an array literal |
//! | `e?` | [`SyntaxKind::TryExpr`] |
//! | `f x y` | application spine, one `Arg` per operand |
//!
//! A grouped argument may carry a passing convention: `(inout pair.0)`.

use rill_diagnostics::{Diagnostic, Diagnostics};
use rill_span::{BytePos, FileId, SourceMap, Span};
use rill_syntax::{Cst, Element, SyntaxKind, SyntaxNode, SyntaxToken};

pub mod decl;
pub mod expr;
pub mod stmt;
pub mod types;

use SyntaxKind as K;

/// Result of parsing one file.
pub struct Parse {
    pub cst: Cst,
    pub diagnostics: Diagnostics,
}

/// Lexes and parses the contents of `file`.
pub fn parse_source(source_map: &SourceMap, file: FileId) -> Parse {
    let src = source_map.file(file);
    let (tokens, mut diagnostics) = rill_lexer::tokenize(&src.text, src.start);
    let parsed = parse_tokens(&tokens, Some((&src.text, src.start)));
    diagnostics.extend(parsed.diagnostics);
    Parse {
        cst: parsed.cst,
        diagnostics,
    }
}

/// Parses an already-lexed token stream without access to the source text;
/// error messages then quote token kinds rather than token text.
pub fn parse_file(tokens: &[SyntaxToken]) -> Parse {
    parse_tokens(tokens, None)
}

/// Parses tokens, optionally with the source text backing them (for messages).
pub fn parse_tokens(tokens: &[SyntaxToken], source: Option<(&str, BytePos)>) -> Parse {
    let mut p = Parser::new(tokens, source);
    p.root();
    let cst = build(tokens, &p.events);
    Parse {
        cst: Cst::new(cst),
        diagnostics: p.diagnostics,
    }
}

/// Parses `text` standalone, returning the parse and the `SourceMap` owning it.
/// Useful in tests; [`parse_source`] is the entry point for real files.
pub fn parse_text(name: &str, text: &str) -> (Parse, SourceMap) {
    let mut sm = SourceMap::new();
    let fid = sm.add_file(name, text.to_string());
    let parse = parse_source(&sm, fid);
    (parse, sm)
}

// ---------------------------------------------------------------------------
// Events and tree building
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Event {
    /// Opens a node; `at` is the index of the parser's current token when the
    /// node started, so leading trivia can be attached to the parent.
    Start {
        kind: SyntaxKind,
        at: usize,
    },
    Finish,
    /// Index of a consumed non-trivia token in the full token vector.
    Token(usize),
}

/// Replays the event stream into a lossless CST.
fn build(tokens: &[SyntaxToken], events: &[Event]) -> SyntaxNode {
    struct Frame {
        kind: SyntaxKind,
        children: Vec<Element>,
    }

    let mut stack = vec![Frame {
        kind: K::Root,
        children: Vec::new(),
    }];
    let mut next_unclaimed = 0usize;

    for event in events {
        match event {
            Event::Start { kind, at } => {
                // Trivia before the node's first token belongs to the parent.
                let frame = stack.last_mut().expect("node outside of any node");
                let from = next_unclaimed.min(*at);
                for token in &tokens[from..*at] {
                    debug_assert!(token.is_trivia(), "parser skipped a real token");
                    frame.children.push(Element::Token(*token));
                }
                if from < *at {
                    next_unclaimed = *at;
                }
                stack.push(Frame {
                    kind: *kind,
                    children: Vec::new(),
                });
            }
            Event::Finish => {
                let frame = stack.pop().expect("unbalanced Finish event");
                let node = SyntaxNode::new(frame.kind, frame.children);
                stack
                    .last_mut()
                    .expect("Finish without an open node")
                    .children
                    .push(Element::Node(node));
            }
            Event::Token(idx) => {
                debug_assert!(!tokens[*idx].is_trivia(), "parser consumed a trivia token");
                let frame = stack.last_mut().expect("token outside of any node");
                for token in &tokens[next_unclaimed..*idx] {
                    debug_assert!(token.is_trivia(), "parser skipped a real token");
                    frame.children.push(Element::Token(*token));
                }
                frame.children.push(Element::Token(tokens[*idx]));
                next_unclaimed = idx + 1;
            }
        }
    }

    let mut root = stack.pop().expect("missing root frame");
    debug_assert!(stack.is_empty(), "unbalanced Start events");
    // Anything left (trailing trivia, the Eof token) belongs to the root.
    for token in &tokens[next_unclaimed..] {
        root.children.push(Element::Token(*token));
    }
    SyntaxNode::new(root.kind, root.children)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct Parser<'a> {
    tokens: &'a [SyntaxToken],
    /// Source text backing `tokens`, for diagnostics.
    source: Option<(&'a str, BytePos)>,
    /// Index of the next *non-trivia* token.
    pos: usize,
    /// End of the last consumed token; drives the adjacency rule.
    prev_end: Option<BytePos>,
    /// Kind of the last consumed token.
    last_kind: SyntaxKind,
    /// While non-zero, a `{` in application position terminates the
    /// expression instead of starting a brace argument. Used for the
    /// scrutinee of `match scrutinee { arms }`, where the brace belongs to
    /// the match and not to the scrutinee.
    block_arg_suppressed: u32,
    pub(crate) events: Vec<Event>,
    pub(crate) diagnostics: Diagnostics,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [SyntaxToken], source: Option<(&'a str, BytePos)>) -> Self {
        assert!(
            matches!(tokens.last(), Some(t) if t.kind == K::Eof),
            "token stream must end with Eof"
        );
        let mut parser = Parser {
            tokens,
            source,
            pos: 0,
            prev_end: None,
            last_kind: K::Root,
            block_arg_suppressed: 0,
            events: Vec::new(),
            diagnostics: Diagnostics::new(),
        };
        parser.skip_trivia();
        parser
    }

    fn skip_trivia(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].is_trivia() {
            self.pos += 1;
        }
    }

    // -- cursor ------------------------------------------------------------

    /// The current token (never trivia). `Eof` at the end of the stream.
    pub fn peek(&self) -> SyntaxToken {
        self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    pub fn kind(&self) -> SyntaxKind {
        self.peek().kind
    }

    pub fn at(&self, kind: SyntaxKind) -> bool {
        self.kind() == kind
    }

    pub fn at_end(&self) -> bool {
        self.kind() == K::Eof
    }

    /// Kind of the `n`-th non-trivia token from the cursor (`n == 0` is `peek`).
    pub fn nth(&self, n: usize) -> SyntaxKind {
        let mut seen = 0usize;
        for tok in &self.tokens[self.pos..] {
            if tok.is_trivia() {
                continue;
            }
            if seen == n {
                return tok.kind;
            }
            seen += 1;
        }
        K::Eof
    }

    /// The `n`-th non-trivia token from the cursor.
    pub fn nth_token(&self, n: usize) -> SyntaxToken {
        let mut seen = 0usize;
        for tok in &self.tokens[self.pos..] {
            if tok.is_trivia() {
                continue;
            }
            if seen == n {
                return *tok;
            }
            seen += 1;
        }
        *self.tokens.last().unwrap()
    }

    /// Source text of a token, when the source is available.
    pub fn token_text(&self, tok: SyntaxToken) -> &'a str {
        match self.source {
            Some((text, base)) => {
                let start = tok.span.start.0.saturating_sub(base.0) as usize;
                let end = tok.span.end.0.saturating_sub(base.0) as usize;
                text.get(start..end).unwrap_or("")
            }
            None => tok.kind.token_text().unwrap_or(""),
        }
    }

    /// Text of the current token.
    pub fn current_text(&self) -> &'a str {
        self.token_text(self.peek())
    }

    /// True while brace arguments are suppressed (see `block_arg_suppressed`).
    pub fn block_args_suppressed(&self) -> bool {
        self.block_arg_suppressed > 0
    }

    /// Runs `f` with brace arguments suppressed.
    pub fn with_no_block_args<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.block_arg_suppressed += 1;
        let result = f(self);
        self.block_arg_suppressed -= 1;
        result
    }

    /// Index of the next non-trivia token; used for progress checks.
    pub fn pos_marker(&self) -> usize {
        self.pos
    }

    /// Kind of the most recently consumed token (defaults to `Root`).
    pub fn last_kind(&self) -> SyntaxKind {
        self.last_kind
    }

    /// True when the current token touches the previously consumed token
    /// (no trivia in between) — the adjacency rule of §2.5.
    pub fn adjacent(&self) -> bool {
        match self.prev_end {
            Some(end) => end == self.peek().span.start,
            None => false,
        }
    }

    pub fn span(&self) -> Span {
        let tok = self.peek();
        if tok.kind == K::Eof {
            Span::new(tok.span.start, tok.span.start)
        } else {
            tok.span
        }
    }

    /// Consumes the current token, recording it in the CST.
    pub fn bump(&mut self) -> SyntaxToken {
        let tok = self.peek();
        debug_assert!(!tok.is_trivia());
        if tok.kind != K::Eof {
            self.events.push(Event::Token(self.pos));
            self.prev_end = Some(tok.span.end);
            self.last_kind = tok.kind;
            self.pos += 1;
            self.skip_trivia();
        }
        tok
    }

    pub fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes `kind`, reporting `expected <what>` otherwise.
    pub fn expect(&mut self, kind: SyntaxKind, what: &str) -> bool {
        if self.eat(kind) {
            true
        } else {
            self.error(format!("expected {what}"));
            false
        }
    }

    // -- diagnostics -------------------------------------------------------

    pub fn error(&mut self, message: impl Into<String>) {
        let span = self.span();
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    pub fn error_at(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    /// `unexpected token` helper, quoting the token text when available.
    pub fn error_unexpected(&mut self) {
        let tok = self.peek();
        let text = self.token_text(tok);
        let shown = if text.is_empty() {
            format!("{}", tok.kind)
        } else {
            format!("`{text}`")
        };
        self.error_at(tok.span, format!("unexpected token {shown}"));
    }

    // -- events ------------------------------------------------------------

    pub fn start(&mut self, kind: SyntaxKind) {
        let at = self.pos;
        self.start_at(kind, at);
    }

    pub fn start_at(&mut self, kind: SyntaxKind, at: usize) {
        self.events.push(Event::Start { kind, at });
    }

    pub fn finish(&mut self) {
        self.events.push(Event::Finish);
    }

    /// A marker for a node that will be opened *before* the events emitted
    /// from now on (used to build left-associative trees).
    pub fn marker(&self) -> Marker {
        Marker {
            idx: self.events.len(),
            at: self.pos,
        }
    }

    /// Runs `f` inside a node of the given kind.
    pub fn node(&mut self, kind: SyntaxKind, f: impl FnOnce(&mut Self)) -> CompletedMarker {
        let marker = self.marker();
        self.start_at(kind, marker.at);
        f(self);
        self.finish();
        CompletedMarker {
            idx: marker.idx,
            at: marker.at,
        }
    }

    /// Removes the node wrapper most recently produced by [`Parser::node`],
    /// splicing its children into the parent (used for tail expressions).
    pub fn unwrap_node(&mut self, marker: CompletedMarker) {
        debug_assert!(matches!(self.events.last(), Some(Event::Finish)));
        self.events.pop();
        let idx = marker.idx;
        debug_assert!(idx < self.events.len());
        self.events.remove(idx);
    }

    /// Parses the token stream as a module. The `Root` frame itself is created
    /// by [`build`], so no `Start` event is emitted here.
    fn root(&mut self) {
        while !self.at_end() {
            let before = self.pos;
            self.statement();
            if self.pos == before {
                // Guarantee progress on malformed input.
                let span = self.span();
                let text = self.current_text().to_string();
                self.bump();
                self.error_at(span, format!("unexpected token `{text}`"));
            }
        }
    }
}

/// A node that has not been opened yet: when completed it inserts its `Start`
/// event at `idx`, wrapping the events already emitted from that point.
/// `at` is the parser position of the node's first token, which lets the tree
/// builder attach leading trivia to the *parent* rather than to this node.
#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub(crate) idx: usize,
    pub(crate) at: usize,
}

/// A marker whose `Start`/`Finish` pair is already in the event stream.
#[derive(Clone, Copy, Debug)]
pub struct CompletedMarker {
    pub(crate) idx: usize,
    pub(crate) at: usize,
}

impl Marker {
    /// Wraps all events emitted since this marker in a node of `kind`.
    pub fn complete(self, p: &mut Parser<'_>, kind: SyntaxKind) -> CompletedMarker {
        p.events
            .insert(self.idx, Event::Start { kind, at: self.at });
        p.events.push(Event::Finish);
        CompletedMarker {
            idx: self.idx,
            at: self.at,
        }
    }
}

impl CompletedMarker {
    /// Prepares to wrap this node (and anything after it) in a new parent.
    pub fn precede(self, _p: &mut Parser<'_>) -> Marker {
        Marker {
            idx: self.idx,
            at: self.at,
        }
    }
}

impl std::fmt::Debug for Parser<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parser")
            .field("pos", &self.pos)
            .field("events", &self.events.len())
            .finish()
    }
}
