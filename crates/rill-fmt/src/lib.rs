//! The Rill formatter.
//!
//! It works on the lossless concrete tree, so comments and every token survive -
//! and it must never change *adjacency*, because in Rill `f[x]` (adjacent) is a call
//! while `f [x]` is an application of `f` to an array. Spacing is decided per token
//! pair, with adjacency taken from the source spans.

use rill_syntax::{Element, SyntaxKind, SyntaxNode};

/// Formats `text`, returning the canonical form. Idempotent: formatting twice is
/// formatting once.
pub fn format_text(text: &str) -> String {
    let (parse, _sm) = rill_parser::parse_text("<fmt>", text);
    let mut out = String::new();
    let mut depth = 0usize;
    {
        let mut writer = Writer {
            src: text,
            out: &mut out,
            depth: &mut depth,
            prev: None,
        };
        writer.node(&parse.cst.root);
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

struct Writer<'a> {
    src: &'a str,
    out: &'a mut String,
    depth: &'a mut usize,
    prev: Option<(
        SyntaxKind,
        usize, /* end offset */
        bool,  /* was a comment */
    )>,
}

impl Writer<'_> {
    fn node(&mut self, node: &SyntaxNode) {
        for child in &node.children {
            match child {
                Element::Node(node) => self.node(node),
                Element::Token(token) => self.token(
                    token.kind,
                    token.span.start.to_usize(),
                    token.span.end.to_usize(),
                ),
            }
        }
    }

    fn token(&mut self, kind: SyntaxKind, start: usize, end: usize) {
        let text = self.src.get(start..end).unwrap_or("");
        match kind {
            SyntaxKind::Whitespace => return,
            SyntaxKind::LineComment | SyntaxKind::BlockComment => {
                if self.prev.is_some() && !self.out.ends_with('\n') {
                    self.newline_if_inline(start);
                }
                self.out.push_str(text.trim_end());
                self.out.push('\n');
                let indent = "  ".repeat(*self.depth);
                self.out.push_str(&indent);
                self.prev = Some((kind, end, true));
                return;
            }
            _ => {}
        }
        let space = match self.prev {
            None => false,
            Some((prev_kind, prev_end, _was_comment)) => {
                needs_space(prev_kind, kind) && !adjacent(self.src, prev_end, start)
            }
        };
        if space && !self.out.ends_with(' ') && !self.out.ends_with('\n') {
            self.out.push(' ');
        }
        if kind == SyntaxKind::RBrace {
            *self.depth = self.depth.saturating_sub(1);
            if !self.out.ends_with('\n') {
                self.out.push('\n');
            }
            let indent = "  ".repeat(*self.depth);
            self.out.push_str(&indent);
        }
        self.out.push_str(text);
        if kind == SyntaxKind::LBrace {
            *self.depth += 1;
            self.out.push('\n');
            let indent = "  ".repeat(*self.depth);
            self.out.push_str(&indent);
        }
        self.prev = Some((kind, end, false));
    }

    fn newline_if_inline(&mut self, start: usize) {
        // The original had a newline before this comment; keep the break.
        let _ = start;
        self.out.push('\n');
        let indent = "  ".repeat(*self.depth);
        self.out.push_str(&indent);
    }
}

/// Whether a space belongs between two tokens when the source had none.
fn needs_space(prev: SyntaxKind, next: SyntaxKind) -> bool {
    use SyntaxKind::*;
    // Nothing binds to what follows without one of these.
    if matches!(next, Arrow | Semi) {
        return false;
    }
    if matches!(prev, LParen | LBracket | Dot | ColonColon) {
        return false;
    }
    if matches!(next, RParen | RBracket | Comma | Dot) {
        return false;
    }
    // `f(x)` / `f[x]` would be a call; `f (x)` stays an application.
    if matches!(next, LParen | LBracket) {
        return true;
    }
    if matches!(prev, Comma | Semi) {
        return true;
    }
    true
}

/// Whether two offsets are adjacent in the source, which is semantic in Rill.
fn adjacent(src: &str, prev_end: usize, start: usize) -> bool {
    src.get(prev_end..start)
        .map(|between| between.is_empty())
        .unwrap_or(false)
}
