//! Hand-written cursor lexer for Rill (PLAN.md §2.1).
//!
//! Produces *all* tokens, trivia included, so the parser can build a lossless
//! CST. Lexical errors are reported through [`Diagnostics`] and do not stop
//! the scan: an offending character becomes an [`SyntaxKind::Error`] token so
//! the token stream still tiles the source exactly.

use rill_diagnostics::{Diagnostic, Diagnostics};
use rill_span::{BytePos, Span};
use rill_syntax::{SyntaxKind, SyntaxToken};

/// Numeric type suffixes (§2.1).
const NUM_SUFFIXES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f16",
    "f32", "f64",
];

pub struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    base: BytePos,
    diagnostics: Diagnostics,
}

impl<'a> Lexer<'a> {
    pub fn new(text: &'a str, base: BytePos) -> Self {
        Lexer {
            text,
            pos: 0,
            base,
            diagnostics: Diagnostics::new(),
        }
    }

    /// Scans the whole input. The returned vector always ends with an `Eof`
    /// token; concatenating every token's text reproduces the input.
    pub fn tokenize(mut self) -> (Vec<SyntaxToken>, Diagnostics) {
        let mut tokens = Vec::new();
        while self.pos < self.text.len() {
            tokens.push(self.next_token());
        }
        tokens.push(SyntaxToken::new(
            SyntaxKind::Eof,
            self.span(self.pos, self.pos),
        ));
        (tokens, self.diagnostics)
    }

    // -- cursor ------------------------------------------------------------

    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        let mut chars = self.rest().chars();
        chars.next();
        chars.next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, expected: &str) -> bool {
        if self.rest().starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(
            BytePos(self.base.0 + start as u32),
            BytePos(self.base.0 + end as u32),
        )
    }

    fn token(&self, kind: SyntaxKind, start: usize) -> SyntaxToken {
        SyntaxToken::new(kind, self.span(start, self.pos))
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    // -- dispatch ----------------------------------------------------------

    fn next_token(&mut self) -> SyntaxToken {
        let start = self.pos;
        let Some(c) = self.peek() else {
            return self.token(SyntaxKind::Eof, start);
        };

        // Trivia: whitespace and both comment styles.
        if c.is_whitespace() {
            return self.whitespace(start);
        }
        if c == '#' {
            return self.line_comment(start);
        }
        if c == '/' && self.peek_second() == Some('/') {
            return self.line_comment(start);
        }
        if c == '/' && self.peek_second() == Some('*') {
            return self.block_comment(start);
        }

        // Literals.
        if c == '"' {
            return self.text_literal(start);
        }
        if c == '\'' {
            return self.char_literal(start);
        }
        if c == 'r' && self.peek_second() == Some('#') && self.rest()[2..].starts_with('"') {
            return self.raw_text_literal(start);
        }
        if c.is_ascii_digit() {
            return self.number(start);
        }

        // Identifiers and keywords.
        if is_ident_start(c) {
            return self.identifier(start);
        }

        // Punctuation.
        if is_punct(c) {
            return self.punct(start);
        }

        // Unknown character: consume one char and report.
        let bad = self.bump().unwrap();
        let span = self.span(start, self.pos);
        self.error(span, format!("unexpected character `{bad}`"));
        self.token(SyntaxKind::Error, start)
    }

    // -- trivia ------------------------------------------------------------

    fn whitespace(&mut self, start: usize) -> SyntaxToken {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        self.token(SyntaxKind::Whitespace, start)
    }

    /// Both `#` and `//` introduce a line comment (§2.1).
    fn line_comment(&mut self, start: usize) -> SyntaxToken {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
        self.token(SyntaxKind::LineComment, start)
    }

    /// Nestable block comments (§2.1).
    fn block_comment(&mut self, start: usize) -> SyntaxToken {
        self.bump(); // '/'
        self.bump(); // '*'
        let mut depth = 1usize;
        while depth > 0 {
            if self.eat_str("*/") {
                depth -= 1;
            } else if self.eat_str("/*") {
                depth += 1;
            } else if self.bump().is_none() {
                let span = self.span(start, self.pos);
                self.error(span, "unterminated block comment");
                break;
            }
        }
        self.token(SyntaxKind::BlockComment, start)
    }

    // -- literals ----------------------------------------------------------

    fn text_literal(&mut self, start: usize) -> SyntaxToken {
        self.bump(); // opening quote
        loop {
            match self.bump() {
                None => {
                    let span = self.span(start, self.pos);
                    self.error(span, "unterminated text literal");
                    break;
                }
                Some('"') => break,
                Some('\\') => {
                    if self.bump().is_none() {
                        let span = self.span(start, self.pos);
                        self.error(span, "unterminated escape sequence");
                        break;
                    }
                }
                Some(_) => {}
            }
        }
        self.token(SyntaxKind::Text, start)
    }

    /// `r#"..."#` (§2.1) — the closing `"#` is required verbatim.
    fn raw_text_literal(&mut self, start: usize) -> SyntaxToken {
        self.bump(); // 'r'
        self.bump(); // '#'
        self.bump(); // '"'
        let mut closed = false;
        while self.pos < self.text.len() {
            if self.eat_str("\"#") {
                closed = true;
                break;
            }
            self.bump();
        }
        if !closed {
            let span = self.span(start, self.pos);
            self.error(span, "unterminated raw text literal");
        }
        self.token(SyntaxKind::RawText, start)
    }

    fn char_literal(&mut self, start: usize) -> SyntaxToken {
        self.bump(); // opening quote
        let mut closed = false;
        loop {
            match self.bump() {
                None => break,
                Some('\'') => {
                    closed = true;
                    break;
                }
                Some('\\') => {
                    self.bump();
                }
                Some('\n') => break,
                Some(_) => {}
            }
        }
        if !closed {
            let span = self.span(start, self.pos);
            self.error(span, "unterminated character literal");
        }
        self.token(SyntaxKind::CharLiteral, start)
    }

    fn number(&mut self, start: usize) -> SyntaxToken {
        let mut is_float = false;
        if self.eat_str("0x") || self.eat_str("0X") {
            self.digits(16);
        } else if self.eat_str("0b") || self.eat_str("0B") {
            self.digits(2);
        } else {
            self.digits(10);
            // Fraction: only when a digit follows the dot, so `1..5` lexes as
            // `1` `..` `5` and `1.f` stays `1` `.` `f`.
            if self.peek() == Some('.') && self.peek_second().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
                self.digits(10);
                is_float = true;
            }
        }

        // Optional type suffix; anything identifier-like must be a valid suffix.
        let suffix_start = self.pos;
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        if self.pos > suffix_start {
            let suffix = &self.text[suffix_start..self.pos];
            if NUM_SUFFIXES.contains(&suffix) {
                if suffix.starts_with('f') {
                    is_float = true;
                }
            } else {
                let span = self.span(suffix_start, self.pos);
                self.error(span, format!("invalid numeric suffix `{suffix}`"));
            }
        }

        let kind = if is_float {
            SyntaxKind::FloatLiteral
        } else {
            SyntaxKind::IntLiteral
        };
        self.token(kind, start)
    }

    /// Consumes digits (and `_` separators) in the given radix. `_` is allowed
    /// anywhere between digits (`digit+ ("_" digit+)*`).
    fn digits(&mut self, radix: u32) {
        while let Some(c) = self.peek() {
            if c == '_' || c.is_digit(radix) {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn identifier(&mut self, start: usize) -> SyntaxToken {
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        let text = &self.text[start..self.pos];
        let kind = if text == "_" {
            SyntaxKind::Underscore
        } else if let Some(kw) = SyntaxKind::from_keyword(text) {
            kw
        } else {
            SyntaxKind::Name
        };
        self.token(kind, start)
    }

    // -- punctuation -------------------------------------------------------

    fn punct(&mut self, start: usize) -> SyntaxToken {
        let c = self.bump().unwrap();
        let kind = match c {
            ';' => SyntaxKind::Semi,
            ',' => SyntaxKind::Comma,
            ':' => {
                if self.eat(':') {
                    SyntaxKind::ColonColon
                } else if self.eat('=') {
                    SyntaxKind::ColonEq
                } else {
                    SyntaxKind::Colon
                }
            }
            '.' => {
                if self.eat('.') {
                    if self.eat('<') {
                        SyntaxKind::DotDotLt
                    } else {
                        SyntaxKind::DotDot
                    }
                } else {
                    SyntaxKind::Dot
                }
            }
            '-' => {
                if self.eat('>') {
                    SyntaxKind::Arrow
                } else {
                    SyntaxKind::Minus
                }
            }
            '=' => {
                if self.eat('=') {
                    SyntaxKind::EqEq
                } else {
                    SyntaxKind::Assign
                }
            }
            '!' => {
                if self.eat('=') {
                    SyntaxKind::BangEq
                } else {
                    SyntaxKind::Bang
                }
            }
            '<' => {
                if self.eat('=') {
                    SyntaxKind::LtEq
                } else if self.eat('<') {
                    SyntaxKind::Shl
                } else {
                    SyntaxKind::Lt
                }
            }
            '>' => {
                if self.eat('=') {
                    SyntaxKind::GtEq
                } else if self.eat('>') {
                    SyntaxKind::Shr
                } else {
                    SyntaxKind::Gt
                }
            }
            '+' => {
                if self.eat('+') {
                    SyntaxKind::PlusPlus
                } else {
                    SyntaxKind::Plus
                }
            }
            '*' => SyntaxKind::Star,
            '/' => SyntaxKind::Slash,
            '%' => SyntaxKind::Percent,
            '&' => SyntaxKind::Amp,
            '|' => {
                if self.eat('&') {
                    SyntaxKind::PipeAmp
                } else {
                    SyntaxKind::Pipe
                }
            }
            '^' => SyntaxKind::Caret,
            '(' => SyntaxKind::LParen,
            ')' => SyntaxKind::RParen,
            '[' => SyntaxKind::LBracket,
            ']' => SyntaxKind::RBracket,
            '{' => SyntaxKind::LBrace,
            '}' => SyntaxKind::RBrace,
            '?' => SyntaxKind::Question,
            '@' => SyntaxKind::At,
            _ => unreachable!("is_punct guard"),
        };
        self.token(kind, start)
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn is_punct(c: char) -> bool {
    matches!(
        c,
        ';' | ','
            | ':'
            | '.'
            | '-'
            | '='
            | '!'
            | '<'
            | '>'
            | '+'
            | '*'
            | '/'
            | '%'
            | '&'
            | '|'
            | '^'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '?'
            | '@'
    )
}

/// Convenience entry point: lex `text` whose byte 0 lives at global `base`.
pub fn tokenize(text: &str, base: BytePos) -> (Vec<SyntaxToken>, Diagnostics) {
    Lexer::new(text, base).tokenize()
}

/// Token kinds of the non-trivia tokens, ignoring the trailing `Eof`.
pub fn token_kinds(tokens: &[SyntaxToken]) -> Vec<SyntaxKind> {
    tokens
        .iter()
        .filter(|t| !t.is_trivia() && t.kind != SyntaxKind::Eof)
        .map(|t| t.kind)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_span::SourceMap;

    /// Lexes and returns non-trivia kinds for assertions.
    fn kinds(src: &str) -> Vec<SyntaxKind> {
        let (tokens, diags) = tokenize(src, BytePos::new(0));
        assert!(
            !diags.has_errors(),
            "unexpected lex errors: {:?}",
            diags.items()
        );
        token_kinds(&tokens)
    }

    /// Lexes and returns `(kind, text)` for non-trivia tokens.
    fn toks(src: &str) -> Vec<(SyntaxKind, String)> {
        let (tokens, diags) = tokenize(src, BytePos::new(0));
        assert!(
            !diags.has_errors(),
            "unexpected lex errors: {:?}",
            diags.items()
        );
        tokens
            .iter()
            .filter(|t| !t.is_trivia() && t.kind != SyntaxKind::Eof)
            .map(|t| {
                let span = t.span;
                let start = span.start.to_usize();
                let end = span.end.to_usize();
                (t.kind, src[start..end].to_string())
            })
            .collect()
    }

    #[test]
    fn keywords_vs_contextual_names() {
        use SyntaxKind::*;
        assert_eq!(
            kinds("let x = if a else b"),
            vec![KwLet, Name, Assign, KwIf, Name, KwElse, Name]
        );
        // Contextual identifiers stay names.
        assert_eq!(
            kinds("option all sync race rush branch computes"),
            vec![Name; 7]
        );
        assert_eq!(kinds("_"), vec![Underscore]);
        assert_eq!(kinds("_x"), vec![Name]);
    }

    #[test]
    fn numbers_with_suffixes_and_ranges() {
        use SyntaxKind::*;
        assert_eq!(
            kinds("0xFF 0b1010 500i64 0x10203040u32 1.5f32 4096 1_000_000"),
            vec![
                IntLiteral,
                IntLiteral,
                IntLiteral,
                IntLiteral,
                FloatLiteral,
                IntLiteral,
                IntLiteral
            ]
        );
        // `1..5` must not swallow the range dots.
        assert_eq!(kinds("1..5"), vec![IntLiteral, DotDot, IntLiteral]);
        assert_eq!(kinds("0..<n"), vec![IntLiteral, DotDotLt, Name]);
        assert_eq!(kinds("1."), vec![IntLiteral, Dot]);
        assert_eq!(toks("1.5f32")[0].1, "1.5f32");
    }

    #[test]
    fn invalid_suffix_is_reported() {
        let (_, diags) = tokenize("1abc", BytePos::new(0));
        assert!(diags.has_errors());
        assert!(diags.items()[0].message.contains("invalid numeric suffix"));
    }

    #[test]
    fn operators_use_maximal_munch() {
        use SyntaxKind::*;
        assert_eq!(
            kinds("a |& b | c ..< d .. e -> f ::::"),
            vec![
                Name, PipeAmp, Name, Pipe, Name, DotDotLt, Name, DotDot, Name, Arrow, Name,
                ColonColon, ColonColon
            ]
        );
        assert_eq!(kinds("x := y"), vec![Name, ColonEq, Name]);
        assert_eq!(
            kinds("a == b != c <= d >= e << f >> g"),
            vec![Name, EqEq, Name, BangEq, Name, LtEq, Name, GtEq, Name, Shl, Name, Shr, Name]
        );
        assert_eq!(kinds("++ --"), vec![PlusPlus, Minus, Minus]);
    }

    #[test]
    fn comments_are_trivia_and_nest() {
        use SyntaxKind::*;
        let src = "# hash comment\nx // line\n/* a /* nested */ b */ y";
        let (tokens, diags) = tokenize(src, BytePos::new(0));
        assert!(!diags.has_errors());
        assert_eq!(token_kinds(&tokens), vec![Name, Name]);
        let comments = tokens.iter().filter(|t| t.kind == LineComment).count();
        assert_eq!(comments, 2);
        let block = tokens.iter().find(|t| t.kind == BlockComment).unwrap();
        assert_eq!(
            &src[block.span.start.to_usize()..block.span.end.to_usize()],
            "/* a /* nested */ b */"
        );
    }

    #[test]
    fn literals_text_raw_char() {
        use SyntaxKind::*;
        assert_eq!(kinds(r#""hello \" world""#), vec![Text]);
        assert_eq!(kinds("r#\"raw \\n text\"#"), vec![RawText]);
        assert_eq!(kinds("'a' '\\n' '\\''"), vec![CharLiteral; 3]);
        // `r` alone stays a name; `r#"` only starts raw text when a quote follows.
        assert_eq!(kinds("r x"), vec![Name, Name]);
    }

    #[test]
    fn unterminated_literal_reports_but_tiles() {
        let (tokens, diags) = tokenize("\"oops", BytePos::new(0));
        assert!(diags.has_errors());
        assert_eq!(token_kinds(&tokens), vec![SyntaxKind::Text]);
        assert_eq!(tokens[0].span.len(), 5);
    }

    #[test]
    fn tokens_tile_the_source_exactly() {
        let src = "let update (t: inout Transform) <computes> = {\n    # comment\n    rotate_point (inout t.position, t.rotation);\n};\n";
        let mut sm = SourceMap::new();
        let fid = sm.add_file("t.rill", src.to_string());
        let (tokens, diags) = tokenize(src, sm.file(fid).start);
        assert!(!diags.has_errors());
        let rebuilt: String = tokens.iter().map(|t| sm.snippet(t.span)).collect();
        assert_eq!(rebuilt, src);
        // And spans are contiguous.
        for pair in tokens.windows(2) {
            assert_eq!(pair[0].span.end, pair[1].span.start);
        }
    }

    #[test]
    fn unknown_character_is_error_token() {
        let (tokens, diags) = tokenize("x $ y", BytePos::new(0));
        assert!(diags.has_errors());
        assert_eq!(
            token_kinds(&tokens),
            vec![SyntaxKind::Name, SyntaxKind::Error, SyntaxKind::Name]
        );
    }
}
