//! Token-only call context and bounded unfinished-caller recovery.
use crate::{
    lexer::Kind,
    source::{LexemeKind, SourceFile},
};
use std::ops::Range;

pub(super) struct Call {
    pub(super) name: String,
    pub(super) callee: Range<usize>,
    pub(super) argument: usize,
    pub(super) open: usize,
}

// Parentheses inside strings/comments are absent from this token stream. Only
// commas at the call's own delimiter depth advance its active parameter.
pub(super) fn active_call(source: &str, byte: usize) -> Option<Call> {
    let lexical = SourceFile::lex(source).ok()?;
    if lexical.lexemes().iter().any(|p| match p.kind() {
        LexemeKind::LineComment => p.span().start.byte <= byte && byte <= p.span().end.byte,
        LexemeKind::Token(Kind::Str(_) | Kind::CStr(_)) => {
            p.span().start.byte < byte && byte < p.span().end.byte
        }
        _ => false,
    }) {
        return None;
    }
    let tokens: Vec<_> = lexical
        .lexemes()
        .iter()
        .filter_map(|p| match p.kind() {
            LexemeKind::Token(kind) if p.span().start.byte < byte => Some((kind, p.span())),
            _ => None,
        })
        .collect();
    let mut stack: Vec<(bool, Option<Call>)> = vec![];
    for (index, (kind, span)) in tokens.iter().enumerate() {
        match kind {
            Kind::LParen => {
                let call = index.checked_sub(1).and_then(|previous| {
                    let (name, end) = match tokens[previous] {
                        (Kind::Ident(name), end) => (name.clone(), end),
                        (Kind::RParen, end) => {
                            return Some(Call {
                                name: String::new(),
                                callee: end.byte_range(),
                                argument: 0,
                                open: span.start.byte,
                            })
                        }
                        _ => return None,
                    };
                    if previous > 0 && matches!(tokens[previous - 1].0, Kind::Fn) {
                        return None;
                    }
                    let mut name = name;
                    let mut first = previous;
                    let mut segments = 0;
                    while first >= 2 && matches!(tokens[first - 1].0, Kind::Dot) {
                        let (Kind::Ident(part), _) = tokens[first - 2] else {
                            return None;
                        };
                        segments += 1;
                        if segments > 16 {
                            return None;
                        }
                        name = format!("{part}.{name}");
                        first -= 2;
                    }
                    let start = tokens[first].1.start.byte;
                    Some(Call {
                        name,
                        callee: start..end.end.byte,
                        argument: 0,
                        open: span.start.byte,
                    })
                });
                stack.push((true, call));
            }
            Kind::LBrace => stack.push((false, None)),
            Kind::RParen => {
                if stack.last().is_some_and(|(paren, _)| *paren) {
                    stack.pop();
                }
            }
            Kind::RBrace => {
                if stack.last().is_some_and(|(paren, _)| !paren) {
                    stack.pop();
                }
            }
            Kind::Comma => {
                if let Some((_, Some(call))) = stack.last_mut() {
                    call.argument += 1;
                }
            }
            _ => {}
        }
    }
    stack.into_iter().rev().find_map(|(_, call)| call)
}

pub(in crate::lsp::analysis) fn recover_caller(
    source: &str,
    byte: usize,
) -> Option<(String, String)> {
    let recovery = Recovery::new(source, byte)?;
    let mut declarations = source.to_owned();
    declarations.replace_range(
        recovery.function.clone(),
        &" ".repeat(recovery.function.len()),
    );
    let scoped = recovery.replace(source, recovery.statement, "()");
    Some((declarations, scoped))
}

// Keep the already complete callee, then terminate its statement. The compiler
// can check its actual result without fabricating any missing call arguments.
pub(super) fn recover_callee(source: &str, open: usize) -> Option<String> {
    Some(Recovery::new(source, open)?.replace(source, open, ";()"))
}

struct Recovery {
    function: Range<usize>,
    statement: usize,
    closing: usize,
}

impl Recovery {
    fn new(source: &str, byte: usize) -> Option<Self> {
        let lexical = SourceFile::lex(source).ok()?;
        let tokens: Vec<_> = lexical
            .lexemes()
            .iter()
            .filter(|p| matches!(p.kind(), LexemeKind::Token(_)))
            .collect();
        let mut function = None;
        let mut blocks = Vec::new();
        let mut statement = None;
        for (index, token) in tokens.iter().enumerate() {
            if token.span().start.byte >= byte {
                break;
            }
            if blocks.is_empty() && matches!(token.kind(), LexemeKind::Token(Kind::Fn)) {
                function = Some(index);
            }
            match token.kind() {
                LexemeKind::Token(Kind::LBrace) => {
                    blocks.push((index, statement));
                    statement = Some(token.span().end.byte);
                }
                LexemeKind::Token(Kind::RBrace) => {
                    if let Some((_, previous)) = blocks.pop() {
                        statement = previous;
                    }
                }
                _ => {}
            }
            if token.span().end.byte <= byte
                && matches!(token.kind(), LexemeKind::Token(Kind::Semi))
            {
                statement = Some(token.span().end.byte);
            }
        }
        let start = function?;
        let body = (start..tokens.len())
            .find(|&index| matches!(tokens[index].kind(), LexemeKind::Token(Kind::LBrace)))?;
        let block = blocks.last()?.0;
        let close = |open: usize| {
            let mut depth = 1usize;
            (open + 1..tokens.len()).find(|&index| {
                match tokens[index].kind() {
                    LexemeKind::Token(Kind::LBrace) => depth += 1,
                    LexemeKind::Token(Kind::RBrace) => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
        };
        let end = close(body).map_or(source.len(), |index| tokens[index].span().end.byte);
        let closing = close(block).map_or(source.len(), |index| tokens[index].span().start.byte);
        let mut from = tokens[start].span().start.byte;
        if start > 0
            && matches!(tokens[start - 1].kind(), LexemeKind::Token(Kind::Ident(word)) if word == "export")
        {
            from = tokens[start - 1].span().start.byte;
        }
        let statement = statement?;
        (statement < closing).then_some(Self {
            function: from..end,
            statement,
            closing,
        })
    }

    fn replace(&self, source: &str, start: usize, value: &str) -> String {
        let mut scoped = source.to_owned();
        scoped.replace_range(
            start..self.closing,
            &format!(
                "{value}{}",
                " ".repeat((self.closing - start).saturating_sub(value.len()))
            ),
        );
        // Existing outer block/lambda closers remain in place. At EOF, close
        // only delimiters opened by the preserved source prefix.
        if let Ok(lexical) = SourceFile::lex(&scoped) {
            let mut delimiters = Vec::new();
            for token in lexical.lexemes() {
                match token.kind() {
                    LexemeKind::Token(Kind::LBrace) => delimiters.push('}'),
                    LexemeKind::Token(Kind::LParen) => delimiters.push(')'),
                    LexemeKind::Token(Kind::RBrace) if delimiters.last() == Some(&'}') => {
                        delimiters.pop();
                    }
                    LexemeKind::Token(Kind::RParen) if delimiters.last() == Some(&')') => {
                        delimiters.pop();
                    }
                    _ => {}
                }
            }
            scoped.extend(delimiters.into_iter().rev());
        }
        scoped
    }
}
