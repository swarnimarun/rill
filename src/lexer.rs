//! Lexer for the supported Rill subset.

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

pub fn lex(source: &str) -> Result<Vec<Token>, String> {
    let mut chars = source.chars().peekable();
    let (mut line, mut column) = (1, 1);
    let mut tokens = Vec::new();
    while let Some(c) = chars.next() {
        let (start_line, start_column) = (line, column);
        column += 1;
        if c == '\n' {
            line += 1;
            column = 1;
            continue;
        }
        if c.is_whitespace() {
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    line += 1;
                    column = 1;
                    break;
                }
                column += 1;
            }
            continue;
        }
        let kind = if c == '"' || (c == 'c' && chars.peek() == Some(&'"')) {
            let is_c = c == 'c';
            if is_c {
                chars.next();
                column += 1;
            }
            let mut value = String::new();
            loop {
                let ch = chars
                    .next()
                    .ok_or_else(|| format!("{start_line}:{start_column}: unterminated string"))?;
                column += 1;
                match ch {
                    '"' => break,
                    '\n' | '\r' => {
                        return Err(format!("{start_line}:{start_column}: newline in string"))
                    }
                    '\0' if is_c => {
                        return Err(format!("{start_line}:{start_column}: NUL in C string"))
                    }
                    '\\' => {
                        let escaped = chars.next().ok_or_else(|| {
                            format!("{start_line}:{start_column}: unterminated string escape")
                        })?;
                        column += 1;
                        match escaped {
                            'n' => value.push('\n'),
                            'r' => value.push('\r'),
                            't' => value.push('\t'),
                            '0' if !is_c => value.push('\0'),
                            '\\' => value.push('\\'),
                            '"' => value.push('"'),
                            _ => {
                                return Err(format!(
                                    "{start_line}:{start_column}: unsupported string escape"
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
                column += 1;
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
                column += 1;
            }
            Kind::Int(s.parse().map_err(|_| {
                format!("{start_line}:{start_column}: integer literal out of i32 range")
            })?)
        } else {
            let mut paired = |expected| {
                if chars.peek() == Some(&expected) {
                    chars.next();
                    column += 1;
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
                    return Err(format!(
                        "{start_line}:{start_column}: unexpected character {c:?}"
                    ))
                }
            }
        };
        tokens.push(Token {
            kind,
            line: start_line,
            column: start_column,
        });
    }
    tokens.push(Token {
        kind: Kind::Eof,
        line,
        column,
    });
    Ok(tokens)
}
