//! Lexer for the supported Rill subset.
use crate::source::{Lexeme, LexemeKind, Position, SourceFile, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Ident(String),
    Int(i64),
    Str(String),
    CStr(String),
    Let,
    Mut,
    Move,
    Inout,
    Read,
    Record,
    Extern,
    Const,
    At,
    Fn,
    Type,
    Match,
    While,
    Do,
    In,
    If,
    Then,
    Else,
    True,
    False,
    LBrace,
    RBrace,
    Pipe,
    Arrow,
    ThinArrow,
    Assign,
    Colon,
    Dot,
    LParen,
    RParen,
    Comma,
    Semi,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: Kind,
    pub line: usize,
    pub column: usize,
}

/// The parser's historical significant-token API, using the same scanner as
/// the lossless source representation.
pub fn lex(source: &str) -> Result<Vec<Token>, String> {
    lex_lossless(source).map(SourceFile::into_tokens)
}

pub fn lex_lossless(source: &str) -> Result<SourceFile<'_>, String> {
    lex_diagnostic(source).map_err(|error| error.message)
}
pub(crate) struct LexError {
    pub span: Span,
    pub message: String,
}
pub(crate) fn lex_diagnostic(source: &str) -> Result<SourceFile<'_>, LexError> {
    let mut chars = Cursor::new(source);
    let mut lexemes = Vec::new();
    while chars.peek().is_some() {
        let start = chars.position;
        let (start_line, start_column) = (start.line, start.column);
        let error = |end, message| LexError {
            span: Span { start, end },
            message,
        };
        let c = chars.next().unwrap();
        if c.is_whitespace() {
            while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
                chars.next();
            }
            lexemes.push(Lexeme::new(
                LexemeKind::Whitespace,
                Span {
                    start,
                    end: chars.position,
                },
                source,
            ));
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            while chars.peek().is_some_and(|ch| *ch != '\n') {
                chars.next();
            }
            lexemes.push(Lexeme::new(
                LexemeKind::LineComment,
                Span {
                    start,
                    end: chars.position,
                },
                source,
            ));
            continue;
        }
        let kind = if c == '"' || (c == 'c' && chars.peek() == Some(&'"')) {
            let is_c = c == 'c';
            if is_c {
                chars.next();
            }
            let mut value = String::new();
            loop {
                let ch = chars.next().ok_or_else(|| {
                    error(
                        chars.position,
                        format!("{start_line}:{start_column}: unterminated string"),
                    )
                })?;
                match ch {
                    '"' => break,
                    '\n' | '\r' => {
                        return Err(error(
                            chars.position,
                            format!("{start_line}:{start_column}: newline in string"),
                        ))
                    }
                    '\0' if is_c => {
                        return Err(error(
                            chars.position,
                            format!("{start_line}:{start_column}: NUL in C string"),
                        ))
                    }
                    '\\' => {
                        let escaped = chars.next().ok_or_else(|| {
                            error(
                                chars.position,
                                format!("{start_line}:{start_column}: unterminated string escape"),
                            )
                        })?;
                        match escaped {
                            'n' => value.push('\n'),
                            'r' => value.push('\r'),
                            't' => value.push('\t'),
                            '0' if !is_c => value.push('\0'),
                            '\\' => value.push('\\'),
                            '"' => value.push('"'),
                            _ => {
                                return Err(error(
                                    chars.position,
                                    format!(
                                        "{start_line}:{start_column}: unsupported string escape"
                                    ),
                                ))
                            }
                        }
                    }
                    _ => value.push(ch),
                }
            }
            if is_c {
                Kind::CStr(value)
            } else {
                Kind::Str(value)
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::from(c);
            while chars
                .peek()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            {
                s.push(chars.next().unwrap());
            }
            match s.as_str() {
                "let" => Kind::Let,
                "mut" => Kind::Mut,
                "move" => Kind::Move,
                "inout" => Kind::Inout,
                "read" => Kind::Read,
                "record" => Kind::Record,
                "extern" => Kind::Extern,
                "const" => Kind::Const,
                "fn" => Kind::Fn,
                "type" => Kind::Type,
                "match" => Kind::Match,
                "while" => Kind::While,
                "do" => Kind::Do,
                "in" => Kind::In,
                "if" => Kind::If,
                "then" => Kind::Then,
                "else" => Kind::Else,
                "true" => Kind::True,
                "false" => Kind::False,
                _ => Kind::Ident(s),
            }
        } else if c.is_ascii_digit() {
            let mut s = String::from(c);
            while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                s.push(chars.next().unwrap());
            }
            Kind::Int(s.parse().map_err(|_| {
                error(
                    chars.position,
                    format!("{start_line}:{start_column}: integer literal out of i32 range"),
                )
            })?)
        } else {
            let mut paired = |expected| {
                if chars.peek() == Some(&expected) {
                    chars.next();
                    true
                } else {
                    false
                }
            };
            match c {
                '@' => Kind::At,
                '{' => Kind::LBrace,
                '}' => Kind::RBrace,
                '(' => Kind::LParen,
                ')' => Kind::RParen,
                ',' => Kind::Comma,
                ':' => Kind::Colon,
                '.' => Kind::Dot,
                ';' => Kind::Semi,
                '+' => Kind::Plus,
                '-' if paired('>') => Kind::ThinArrow,
                '-' => Kind::Minus,
                '*' => Kind::Star,
                '/' => Kind::Slash,
                '%' => Kind::Percent,
                '=' if paired('>') => Kind::Arrow,
                '=' if paired('=') => Kind::EqEq,
                '=' => Kind::Equal,
                '!' if paired('=') => Kind::Ne,
                '<' if paired('-') => Kind::Assign,
                '<' if paired('=') => Kind::Le,
                '<' => Kind::Lt,
                '>' if paired('=') => Kind::Ge,
                '>' => Kind::Gt,
                '&' if paired('&') => Kind::And,
                '|' if paired('|') => Kind::Or,
                '|' => Kind::Pipe,
                _ => {
                    return Err(error(
                        chars.position,
                        format!("{start_line}:{start_column}: unexpected character {c:?}"),
                    ))
                }
            }
        };
        lexemes.push(Lexeme::new(
            LexemeKind::Token(kind),
            Span {
                start,
                end: chars.position,
            },
            source,
        ));
    }
    lexemes.push(Lexeme::new(
        LexemeKind::Token(Kind::Eof),
        Span {
            start: chars.position,
            end: chars.position,
        },
        source,
    ));
    Ok(SourceFile::new(source, lexemes))
}

struct Cursor<'s> {
    chars: std::iter::Peekable<std::str::Chars<'s>>,
    position: Position,
}

impl<'s> Cursor<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            chars: source.chars().peekable(),
            position: Position {
                byte: 0,
                line: 1,
                column: 1,
            },
        }
    }

    fn peek(&mut self) -> Option<&char> {
        self.chars.peek()
    }
}

impl Iterator for Cursor<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        self.position.byte += ch.len_utf8();
        if ch == '\n' {
            self.position.line += 1;
            self.position.column = 1;
        } else {
            self.position.column += 1;
        }
        Some(ch)
    }
}
