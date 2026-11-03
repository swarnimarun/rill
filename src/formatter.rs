//! Conservative whitespace-only formatting for the currently accepted syntax.
//!
//! This is not an AST pretty-printer: original token spellings and comments are
//! retained. Every result is re-lexed and re-parsed before it can be returned.

use crate::{
    lexer::Kind,
    parser,
    source::{Lexeme, LexemeKind, SourceFile},
};

/// Format one syntactically valid module without loading imports, checking types
/// or invoking native tools. No source files are read or changed by this API.
/// Unsupported or malformed syntax returns an error, never partial output.
pub fn format(source: &str) -> Result<String, String> {
    let original = SourceFile::lex(source).map_err(input_error)?;
    let parsed = parser::parse_module(source).map_err(input_error)?;
    let output = layout(&original);
    let formatted = SourceFile::lex(&output)
        .map_err(|error| format!("E-FMT-INVARIANT: formatted output does not lex: {error}"))?;
    if !significant(&original).eq(significant(&formatted)) {
        return Err("E-FMT-INVARIANT: formatting would change tokens or comments".into());
    }
    let reparsed = parser::parse_module(&output)
        .map_err(|error| format!("E-FMT-INVARIANT: formatted output does not parse: {error}"))?;
    if parsed != reparsed {
        return Err("E-FMT-INVARIANT: formatting would change the parsed module".into());
    }
    if layout(&formatted) != output {
        return Err("E-FMT-INVARIANT: formatting would not be idempotent".into());
    }
    Ok(output)
}

fn input_error(error: String) -> String {
    format!("E-FMT-SYNTAX: {error}")
}

fn significant<'a, 's>(
    source: &'a SourceFile<'s>,
) -> impl Iterator<Item = (&'a LexemeKind, &'s str)> {
    source.lexemes().iter().filter_map(|piece| {
        (!matches!(piece.kind(), LexemeKind::Whitespace)).then_some((piece.kind(), piece.text()))
    })
}

#[derive(Clone, Copy)]
enum Gap {
    None,
    Space,
    Lines(usize),
}

fn layout(source: &SourceFile<'_>) -> String {
    let mut output = String::with_capacity(source.text().len());
    let mut previous: Option<&Lexeme<'_>> = None;
    let mut braces = 0usize;
    let mut parens = 0usize;
    for piece in source.lexemes() {
        if matches!(
            piece.kind(),
            LexemeKind::Whitespace | LexemeKind::Token(Kind::Eof)
        ) {
            continue;
        }
        let kind = token(piece);
        if kind == Some(&Kind::RBrace) {
            braces = braces.saturating_sub(1);
        }
        if kind == Some(&Kind::RParen) {
            parens = parens.saturating_sub(1);
        }
        if let Some(prev) = previous {
            let original_gap = &source.text()[prev.span().end.byte..piece.span().start.byte];
            match gap(prev, piece, original_gap, braces) {
                Gap::None => {}
                Gap::Space => output.push(' '),
                Gap::Lines(lines) => {
                    for _ in 0..lines {
                        output.push('\n');
                    }
                    for _ in 0..(braces + parens) {
                        output.push_str("    ");
                    }
                }
            }
        }
        output.push_str(piece.text());
        if kind == Some(&Kind::LBrace) {
            braces += 1;
        }
        if kind == Some(&Kind::LParen) {
            parens += 1;
        }
        previous = Some(piece);
    }
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

fn token<'a>(piece: &'a Lexeme<'_>) -> Option<&'a Kind> {
    match piece.kind() {
        LexemeKind::Token(kind) => Some(kind),
        _ => None,
    }
}

fn gap(previous: &Lexeme<'_>, current: &Lexeme<'_>, original: &str, braces: usize) -> Gap {
    let lines = original.bytes().filter(|byte| *byte == b'\n').count();
    let newline = Gap::Lines(lines.clamp(1, 2));
    if matches!(previous.kind(), LexemeKind::LineComment) {
        // A line comment must never swallow the next token.
        return newline;
    }
    if matches!(current.kind(), LexemeKind::LineComment) {
        // Keep inline comments attached to their preceding token, even after a
        // semicolon or opening brace. Comment contents (including spaces) stay exact.
        return if lines == 0 { Gap::Space } else { newline };
    }
    let prev = token(previous).expect("non-trivia piece");
    let curr = token(current).expect("non-trivia piece");
    if prev == &Kind::LBrace && curr == &Kind::RBrace {
        return Gap::None;
    }
    if prev == &Kind::LBrace || curr == &Kind::RBrace || prev == &Kind::Semi {
        return newline;
    }
    if curr == &Kind::Pipe && braces > 0 {
        return newline;
    }
    if prev == &Kind::RBrace
        && matches!(
            curr,
            Kind::Fn | Kind::Let | Kind::Type | Kind::Extern | Kind::Ident(_)
        )
    {
        return newline;
    }
    // Preserve the historical distinction between adjacent and separated call /
    // parameter parentheses, even though today's parser accepts both as calls.
    if matches!(prev, Kind::Ident(_)) && curr == &Kind::LParen {
        return if original.is_empty() {
            Gap::None
        } else if lines > 0 {
            newline
        } else {
            Gap::Space
        };
    }
    // Retain existing line boundaries (and at most one blank line). A lexical
    // formatter cannot infer an author's desired wrapping of legacy expressions.
    if lines > 0 {
        return newline;
    }
    if matches!(
        curr,
        Kind::Comma | Kind::Semi | Kind::RParen | Kind::Colon | Kind::Dot
    ) || matches!(prev, Kind::LParen | Kind::Dot | Kind::At)
    {
        return Gap::None;
    }
    // Retain unary-minus adjacency. Separating '-' from an adjacent literal is
    // unnecessary, and current source spelling is useful for legacy migration.
    if prev == &Kind::Minus && original.is_empty() {
        return Gap::None;
    }
    Gap::Space
}
