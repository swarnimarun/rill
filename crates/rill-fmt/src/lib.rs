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
            nested: 0,
            // Computed before the tree walk: the walk itself is single-pass.
            inline: inline_braces(text),
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
    /// Parens and brackets suspend statement layout: a `;` inside them belongs to
    /// one expression, not to the end of a statement.
    nested: usize,
    /// Opening braces whose block stays on one line.
    inline: std::collections::HashSet<usize>,
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
                let indent = "\t".repeat(*self.depth);
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
        // At the start of a line the indent is the only whitespace: a token must not
        // be pushed right by a stray space after the tab.
        let at_line_start = self.out.ends_with('\n') || self.out.ends_with('\t');
        if space && !at_line_start && !self.out.ends_with(' ') {
            self.out.push(' ');
        }
        match kind {
            SyntaxKind::LParen | SyntaxKind::LBracket => self.nested += 1,
            SyntaxKind::RParen | SyntaxKind::RBracket => {
                self.nested = self.nested.saturating_sub(1)
            }
            _ => {}
        }
        if kind == SyntaxKind::KwElse {
            // `} else {` reads as one construct, not two.
            while self.out.ends_with(' ') || self.out.ends_with('\n') {
                self.out.pop();
            }
            self.out.push_str(" else ");
            self.prev = Some((kind, end, false));
            return;
        }
        if kind == SyntaxKind::RBrace {
            let inline_close = self
                .inline
                .iter()
                .filter(|open| **open < start)
                .max()
                .map(|open| !self.src[*open + 1..start].contains('}'))
                .unwrap_or(false);
            if inline_close {
                while self.out.ends_with(' ') {
                    self.out.pop();
                }
                self.out.push_str(" }");
                self.prev = Some((kind, end, false));
                return;
            }
            *self.depth = self.depth.saturating_sub(1);
            // Drop the trailing indent; the closing brace carries its own.
            while self.out.ends_with(' ') || self.out.ends_with('\n') {
                self.out.pop();
            }
            self.out.push('\n');
            let indent = "\t".repeat(*self.depth);
            self.out.push_str(&indent);
        }
        self.out.push_str(text);
        if kind == SyntaxKind::LBrace {
            if self.inline.contains(&start) {
                self.out.push(' ');
                self.prev = Some((kind, end, false));
                return;
            }
            *self.depth += 1;
            self.out.push('\n');
            let indent = "\t".repeat(*self.depth);
            self.out.push_str(&indent);
        }
        if kind == SyntaxKind::Semi && self.nested == 0 {
            // One statement per line, which is the layout a reader expects - and a
            // blank line after a top-level item, which separates declarations.
            self.out.push('\n');
            if *self.depth == 0 {
                self.out.push('\n');
            }
            let indent = "\t".repeat(*self.depth);
            self.out.push_str(&indent);
        }
        self.prev = Some((kind, end, false));
    }

    fn newline_if_inline(&mut self, start: usize) {
        // The original had a newline before this comment; keep the break.
        let _ = start;
        self.out.push('\n');
        let indent = "\t".repeat(*self.depth);
        self.out.push_str(&indent);
    }
}

/// Whether a space belongs between two tokens when the source had none.
fn needs_space(prev: SyntaxKind, next: SyntaxKind) -> bool {
    use SyntaxKind::*;
    // Nothing binds to what follows without one of these.
    if next == Semi {
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
fn inline_braces(src: &str) -> std::collections::HashSet<usize> {
    let mut stack: Vec<usize> = Vec::new();
    let mut inline = std::collections::HashSet::new();
    for (at, byte) in src.bytes().enumerate() {
        match byte {
            b'{' => stack.push(at),
            b'}' => {
                let Some(open) = stack.pop() else { continue };
                let body = &src[open + 1..at];
                if !body.contains(';')
                    && !body.contains('{')
                    && !body.contains('}')
                    && !body.contains('\n')
                    && body.trim().len() <= 48
                {
                    inline.insert(open);
                }
            }
            _ => {}
        }
    }
    inline
}

fn adjacent(src: &str, prev_end: usize, start: usize) -> bool {
    src.get(prev_end..start)
        .map(|between| between.is_empty())
        .unwrap_or(false)
}
