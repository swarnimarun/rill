//! Lossless lexical source representation. See `syntax` for grammar trees.
//!
//! Every successful lex retains all UTF-8 source bytes, including whitespace,
//! comments and literal spelling. Invalid tokens return the ordinary lexer error;
//! source tooling can retain the whole unlexed file after a lexical error.

use crate::lexer::{Kind, Token};

/// A UTF-8 byte offset and a one-based line/scalar-column position.
/// LF starts a new line. Tabs and CR each advance one column, as in the lexer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub byte: usize,
    pub line: usize,
    pub column: usize,
}

/// A half-open span. Both byte offsets are UTF-8 character boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Span {
    pub fn byte_range(self) -> std::ops::Range<usize> {
        self.start.byte..self.end.byte
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LexemeKind {
    /// Whole input retained on source-tree lexical failure; never strict lexer output.
    UnlexedSource,
    Token(Kind),
    Whitespace,
    /// Includes `//` and its exact contents, but not the terminating LF.
    LineComment,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lexeme<'s> {
    kind: LexemeKind,
    span: Span,
    text: &'s str,
}

impl<'s> Lexeme<'s> {
    pub(crate) fn new(kind: LexemeKind, span: Span, source: &'s str) -> Self {
        Self {
            kind,
            span,
            text: &source[span.byte_range()],
        }
    }

    pub fn kind(&self) -> &LexemeKind {
        &self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn text(&self) -> &'s str {
        self.text
    }
}

#[derive(Clone, Debug)]
pub struct SourceFile<'s> {
    text: &'s str,
    lexemes: Vec<Lexeme<'s>>,
}

impl<'s> SourceFile<'s> {
    pub fn lex(text: &'s str) -> Result<Self, String> {
        crate::lexer::lex_lossless(text)
    }

    pub(crate) fn new(text: &'s str, lexemes: Vec<Lexeme<'s>>) -> Self {
        Self { text, lexemes }
    }

    pub fn text(&self) -> &'s str {
        self.text
    }

    pub(crate) fn unlexed(text: &'s str) -> Self {
        let start = Position {
            byte: 0,
            line: 1,
            column: 1,
        };
        let mut end = start;
        for ch in text.chars() {
            end.byte += ch.len_utf8();
            if ch == '\n' {
                end.line += 1;
                end.column = 1;
            } else {
                end.column += 1;
            }
        }
        Self::new(
            text,
            vec![
                Lexeme::new(LexemeKind::UnlexedSource, Span { start, end }, text),
                Lexeme::new(LexemeKind::Token(Kind::Eof), Span { start: end, end }, text),
            ],
        )
    }

    /// Ordered contiguous source pieces, ending in a zero-width EOF token.
    pub fn lexemes(&self) -> &[Lexeme<'s>] {
        &self.lexemes
    }

    /// Reconstruct from the pieces, rather than returning the original string.
    pub fn reconstruct(&self) -> String {
        self.lexemes.iter().map(Lexeme::text).collect()
    }

    pub(crate) fn into_tokens(self) -> Vec<Token> {
        self.lexemes
            .into_iter()
            .filter_map(|lexeme| match lexeme.kind {
                LexemeKind::Token(kind) => Some(Token {
                    kind,
                    line: lexeme.span.start.line,
                    column: lexeme.span.start.column,
                }),
                _ => None,
            })
            .collect()
    }
}
