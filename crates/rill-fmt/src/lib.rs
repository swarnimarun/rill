//! The Rill formatter.
//!
//! It works on the lossless concrete tree, so comments and every token survive.
//! Adjacency is *semantic* in Rill — `f[x]` is an adjacent (fail-fast) call
//! while `f [x]` is an application to an array literal — so the tree, not the
//! original spacing, decides it: an [`SyntaxKind::IndexExpr`] prints its bracket
//! list touching its callee, and an application prints a space.
//!
//! Parenthesised argument lists are unambiguous: `f(x)` and `f (x)` splice into
//! the same call, so the canonical form touches the callee — `f(x)` — matching
//! the bracket form. The one exception is a spliced list followed by more
//! arguments (`g (a, b) c`), where the space is what keeps the whole spine one
//! call; that layout is preserved.
//!
//! Layout is width-aware: [`Config`] carries the line width, tab width, and the
//! thresholds at which an `if`/`else` chain or a block is broken over several
//! lines. Long argument lists, parameter lists, arrays, tuples and matches break
//! one element per line.

use rill_syntax::{Element, SyntaxKind, SyntaxNode, SyntaxToken};

pub mod config;

pub use config::Config;

/// Formats `text` with the default [`Config`].
pub fn format_text(text: &str) -> String {
    format_text_with(text, &Config::default())
}

/// Formats `text` with `config`, returning the canonical form. Idempotent:
/// formatting twice is formatting once.
pub fn format_text_with(text: &str, config: &Config) -> String {
    let (parse, _sm) = rill_parser::parse_text("<fmt>", text);
    let mut fmt = Fmt {
        src: text,
        cfg: config,
        out: String::new(),
    };
    fmt.root(&parse.cst.root);
    fmt.finish()
}

/// How the children of a braced body are separated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Top-level items: a blank line between declarations.
    Items,
    /// Trait/impl/builder members: a blank line between members.
    Members,
    /// Statements: keep a source blank line, but never invent one.
    Statements,
}

struct Fmt<'a> {
    src: &'a str,
    cfg: &'a Config,
    out: String,
}

impl Fmt<'_> {
    // -- output primitives -------------------------------------------------

    fn finish(mut self) -> String {
        while self.out.ends_with('\n') || self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        self.out.push('\n');
        self.out
    }

    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    fn column(&self) -> usize {
        let line = self.out.rsplit('\n').next().unwrap_or("");
        line.chars().count()
    }

    fn emit(&mut self, text: &str, depth: usize) {
        if text.is_empty() {
            return;
        }
        if self.at_line_start() {
            for _ in 0..depth {
                self.out.push('\t');
            }
        }
        self.out.push_str(text);
    }

    fn newline(&mut self) {
        while self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    /// Exactly `count` blank lines before whatever comes next.
    fn blank_lines(&mut self, count: usize) {
        if self.out.is_empty() {
            return;
        }
        while self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        for _ in 0..count {
            self.out.push('\n');
        }
    }

    fn width_at(&self, text: &str, depth: usize) -> usize {
        let start = if self.at_line_start() {
            depth * self.cfg.tab_width
        } else {
            self.column()
        };
        start + text.chars().count()
    }

    fn fits(&self, text: &str, depth: usize, budget: usize) -> bool {
        self.width_at(text, depth) <= budget
    }

    // -- source helpers ----------------------------------------------------

    fn token_text(&self, token: &SyntaxToken) -> &str {
        self.src
            .get(token.span.start.to_usize()..token.span.end.to_usize())
            .unwrap_or("")
    }

    fn source_blank_line(&self, prev_end: usize, start: usize) -> bool {
        self.src
            .get(prev_end..start)
            .map(|between| between.matches('\n').count() >= 2)
            .unwrap_or(false)
    }

    fn source_same_line(&self, prev_end: usize, start: usize) -> bool {
        self.src
            .get(prev_end..start)
            .map(|between| !between.contains('\n'))
            .unwrap_or(false)
    }

    /// Children without whitespace or the EOF sentinel.
    fn elements<'n>(&self, node: &'n SyntaxNode) -> Vec<&'n Element> {
        node.children
            .iter()
            .filter(|child| {
                !matches!(
                    child,
                    Element::Token(token)
                        if token.kind == SyntaxKind::Whitespace
                            || token.kind == SyntaxKind::Eof
                            || token.kind == SyntaxKind::DocComment
                )
            })
            .collect()
    }

    fn first_kind(&self, node: &SyntaxNode) -> Option<SyntaxKind> {
        for child in &node.children {
            match child {
                Element::Token(token)
                    if token.kind == SyntaxKind::Whitespace || token.kind == SyntaxKind::Eof =>
                {
                    continue;
                }
                Element::Token(token) => return Some(token.kind),
                Element::Node(inner) => {
                    if let Some(kind) = self.first_kind(inner) {
                        return Some(kind);
                    }
                }
            }
        }
        None
    }

    fn last_kind(&self, node: &SyntaxNode) -> Option<SyntaxKind> {
        for child in node.children.iter().rev() {
            match child {
                Element::Token(token)
                    if token.kind == SyntaxKind::Whitespace || token.kind == SyntaxKind::Eof =>
                {
                    continue;
                }
                Element::Token(token) => return Some(token.kind),
                Element::Node(inner) => {
                    if let Some(kind) = self.last_kind(inner) {
                        return Some(kind);
                    }
                }
            }
        }
        None
    }

    fn first_node<'n>(&self, node: &'n SyntaxNode) -> Option<&'n SyntaxNode> {
        node.children.iter().find_map(Element::as_node)
    }

    fn node_parts<'n>(&self, node: &'n SyntaxNode) -> Vec<&'n SyntaxNode> {
        node.children.iter().filter_map(Element::as_node).collect()
    }

    // -- root and item lists -----------------------------------------------

    fn root(&mut self, node: &SyntaxNode) {
        self.write_items(&node.children, 0, Layout::Items);
    }

    /// Writes children one per line. Each item is rendered into its own
    /// buffer first, so the separators can depend on what the item looks like:
    /// a one-line declaration is grouped with its neighbours, a multi-line one
    /// is surrounded by blank lines.
    fn write_items(&mut self, children: &[Element], depth: usize, layout: Layout) {
        struct Chunk {
            text: String,
            kind: Option<SyntaxKind>,
            comment: bool,
            blank_before: bool,
        }

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut prev_end: Option<usize> = None;
        for child in children {
            match child {
                Element::Token(token)
                    if token.kind == SyntaxKind::Whitespace || token.kind == SyntaxKind::Eof =>
                {
                    continue;
                }
                Element::Token(token) if token.kind == SyntaxKind::Semi => {
                    // A loop's trailing `;` is a sibling of the loop node.
                    if let Some(last) = chunks.last_mut() {
                        last.text.push(';');
                    }
                    prev_end = Some(token.span.end.to_usize());
                }
                Element::Token(token) if token.kind == SyntaxKind::DocComment => {}
                Element::Token(token) if token.kind.is_trivia() => {
                    let start = token.span.start.to_usize();
                    let text = self.token_text(token).trim_end().to_string();
                    let same_line = prev_end.is_some_and(|prev| self.source_same_line(prev, start));
                    if same_line {
                        if let Some(last) = chunks.last_mut() {
                            last.text.push(' ');
                            last.text.push_str(&text);
                        }
                    } else {
                        let blank_before =
                            prev_end.is_some_and(|prev| self.source_blank_line(prev, start));
                        let mut text = text;
                        text.insert_str(0, &"\t".repeat(depth));
                        chunks.push(Chunk {
                            text,
                            kind: None,
                            comment: true,
                            blank_before,
                        });
                    }
                    prev_end = Some(token.span.end.to_usize());
                }
                Element::Token(_) => {
                    // Braces of a body and keywords already written by the
                    // caller (`record`, `view`) are not items.
                }
                Element::Node(node) => {
                    let start = node.span.start.to_usize();
                    let blank_before =
                        prev_end.is_some_and(|prev| self.source_blank_line(prev, start));
                    let text = self.render(node, depth);
                    chunks.push(Chunk {
                        text,
                        kind: Some(node.kind),
                        comment: false,
                        blank_before,
                    });
                    prev_end = Some(node.span.end.to_usize());
                }
            }
        }

        let blank = match layout {
            Layout::Items | Layout::Members => self.cfg.blank_lines_between_items,
            Layout::Statements => usize::from(self.cfg.preserve_blank_lines),
        };
        for (index, chunk) in chunks.iter().enumerate() {
            if index > 0 {
                let previous = &chunks[index - 1];
                let grouped_use = previous.kind == Some(SyntaxKind::UseDecl)
                    && chunk.kind == Some(SyntaxKind::UseDecl);
                let grouped_global = matches!(previous.kind, Some(SyntaxKind::VarStmt))
                    && matches!(chunk.kind, Some(SyntaxKind::VarStmt));
                let separator = if grouped_use || grouped_global {
                    0
                } else if previous.comment && !chunk.blank_before {
                    // A comment run stays attached to the declaration below it.
                    0
                } else if chunk.blank_before {
                    blank
                } else {
                    match layout {
                        Layout::Statements => 0,
                        Layout::Members | Layout::Items => blank,
                    }
                };
                if separator > 0 {
                    self.blank_lines(separator);
                } else if !self.at_line_start() {
                    self.newline();
                }
            }
            self.out.push_str(&chunk.text);
        }
    }

    /// Renders a node into its own buffer at `depth`, so a caller can decide
    /// the layout around it.
    fn render(&self, node: &SyntaxNode, depth: usize) -> String {
        let mut inner = Fmt {
            src: self.src,
            cfg: self.cfg,
            out: String::new(),
        };
        inner.write_node(node, depth);
        inner.out
    }

    // -- flat rendering ----------------------------------------------------

    /// Canonical one-line text for a node, or `None` when it cannot stay on one
    /// line (comments, multi-statement blocks, several statements).
    fn flat(&self, node: &SyntaxNode) -> Option<String> {
        use SyntaxKind::*;
        if node
            .children
            .iter()
            .any(|child| matches!(child, Element::Token(token) if token.kind == DocComment))
        {
            return None;
        }
        match node.kind {
            Root => None,
            Block => self.flat_block(node),
            IfExpr => self.flat_if(node),
            MatchExpr => self.flat_match(node),
            MatchArm => self.flat_arm(node),
            LambdaExpr => self.flat_lambda(node),
            CallExpr => self.flat_call(node),
            IndexExpr => self.flat_index(node),
            ArrayExpr => self.flat_array(node),
            ArgList => self.flat_arg_list(node),
            Arg => self.flat_arg(node),
            TryExpr => {
                let base = self.first_node(node)?;
                Some(format!("{}?", self.flat(base)?))
            }
            BinaryExpr => self.flat_binary(node),
            UnaryExpr => self.flat_unary(node),
            ParenExpr => match self.first_node(node) {
                Some(inner) => Some(format!("({})", self.flat(inner)?)),
                None => Some("()".to_string()),
            },
            TupleExpr => {
                let parts = self.node_parts(node);
                let mut rendered = Vec::new();
                for part in parts {
                    rendered.push(self.flat(part)?);
                }
                Some(format!("({})", rendered.join(", ")))
            }
            EffectRow => {
                let items = self.node_parts(node);
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.flat(item)?);
                }
                Some(format!("<{}>", rendered.join(" | ")))
            }
            TypePtr => self.flat_type_ptr(node),
            AttributeDecl => self.flat_attribute(node),
            Include => self.flat_include(node),
            UseGroup => {
                let names = self.node_parts(node);
                let mut rendered = Vec::new();
                for name in names {
                    rendered.push(self.flat(name)?);
                }
                Some(format!("{{{}}}", rendered.join(", ")))
            }
            LambdaParams => self.flat_lambda_params(node),
            _ => self.flat_generic(node),
        }
    }

    fn flat_generic(&self, node: &SyntaxNode) -> Option<String> {
        let elements = self.elements(node);
        self.flat_elements(&elements)
    }

    /// Flattens a run of elements with canonical spacing.
    fn flat_elements(&self, elements: &[&Element]) -> Option<String> {
        let mut out = String::new();
        let mut prev: Option<SyntaxKind> = None;
        for element in elements {
            let (first, last, text) = match element {
                Element::Token(token) => {
                    if token.kind.is_trivia() {
                        return None;
                    }
                    (token.kind, token.kind, self.token_text(token).to_string())
                }
                Element::Node(inner) => (
                    self.first_kind(inner)?,
                    self.last_kind(inner)?,
                    self.flat(inner)?,
                ),
            };
            if let Some(previous) = prev
                && join_space(previous, first)
            {
                out.push(' ');
            }
            out.push_str(&text);
            prev = Some(last);
        }
        Some(out)
    }

    fn flat_block(&self, node: &SyntaxNode) -> Option<String> {
        let mut inner: Vec<&SyntaxNode> = Vec::new();
        for child in &node.children {
            match child {
                Element::Token(token)
                    if token.kind == SyntaxKind::Whitespace
                        || token.kind == SyntaxKind::Eof
                        || token.kind == SyntaxKind::LBrace
                        || token.kind == SyntaxKind::RBrace => {}
                Element::Token(_) => return None,
                Element::Node(inner_node) => inner.push(inner_node),
            }
        }
        match inner.as_slice() {
            [] => Some("{}".to_string()),
            [only] => {
                let body = self.flat(only)?;
                Some(format!("{{ {body} }}"))
            }
            _ => None,
        }
    }

    fn flat_if(&self, node: &SyntaxNode) -> Option<String> {
        let (condition, then_block, else_branch) = self.if_parts(node)?;
        let mut out = format!("if ({}) {}", self.flat(condition)?, self.flat(then_block)?);
        if let Some(branch) = else_branch {
            out.push_str(" else ");
            out.push_str(&self.flat(branch)?);
        }
        Some(out)
    }

    fn if_parts<'n>(
        &self,
        node: &'n SyntaxNode,
    ) -> Option<(&'n SyntaxNode, &'n SyntaxNode, Option<&'n SyntaxNode>)> {
        let mut condition: Option<&SyntaxNode> = None;
        let mut then_block: Option<&SyntaxNode> = None;
        let mut else_branch: Option<&SyntaxNode> = None;
        let mut after_paren = false;
        for child in &node.children {
            match child {
                Element::Token(token) if token.kind == SyntaxKind::RParen => after_paren = true,
                Element::Token(_) => {}
                Element::Node(inner) if !after_paren => condition = Some(inner),
                Element::Node(inner) if then_block.is_none() => then_block = Some(inner),
                Element::Node(inner) => else_branch = Some(inner),
            }
        }
        Some((condition?, then_block?, else_branch))
    }

    fn flat_match(&self, node: &SyntaxNode) -> Option<String> {
        let (scrutinee, arms) = self.match_parts(node);
        let mut rendered = Vec::new();
        for arm in arms {
            rendered.push(self.flat(arm)?);
        }
        Some(format!(
            "match {} {{ {} }}",
            self.flat(scrutinee?)?,
            rendered.join(", ")
        ))
    }

    fn match_parts<'n>(
        &self,
        node: &'n SyntaxNode,
    ) -> (Option<&'n SyntaxNode>, Vec<&'n SyntaxNode>) {
        let mut scrutinee: Option<&SyntaxNode> = None;
        let mut arms: Vec<&SyntaxNode> = Vec::new();
        for child in &node.children {
            match child {
                Element::Node(inner) if inner.kind == SyntaxKind::MatchArm => arms.push(inner),
                Element::Node(inner) => scrutinee = Some(inner),
                Element::Token(_) => {}
            }
        }
        (scrutinee, arms)
    }

    fn arm_parts<'n>(
        &self,
        node: &'n SyntaxNode,
    ) -> (
        Option<&'n SyntaxNode>,
        Option<&'n SyntaxNode>,
        Option<&'n SyntaxNode>,
    ) {
        let mut pattern: Option<&SyntaxNode> = None;
        let mut guard: Option<&SyntaxNode> = None;
        let mut body: Option<&SyntaxNode> = None;
        let mut saw_if = false;
        let mut saw_arrow = false;
        for child in &node.children {
            match child {
                Element::Token(token) if token.kind == SyntaxKind::KwIf => saw_if = true,
                Element::Token(token) if token.kind == SyntaxKind::Arrow => saw_arrow = true,
                Element::Token(_) => {}
                Element::Node(inner) if !saw_if && !saw_arrow && pattern.is_none() => {
                    pattern = Some(inner)
                }
                Element::Node(inner) if saw_if && !saw_arrow => guard = Some(inner),
                Element::Node(inner) if saw_arrow => body = Some(inner),
                Element::Node(_) => {}
            }
        }
        (pattern, guard, body)
    }

    fn flat_arm(&self, node: &SyntaxNode) -> Option<String> {
        let (pattern, guard, body) = self.arm_parts(node);
        let mut out = self.flat(pattern?)?;
        if let Some(guard) = guard {
            out.push_str(" if ");
            out.push_str(&self.flat(guard)?);
        }
        out.push_str(" -> ");
        out.push_str(&self.flat(body?)?);
        Some(out)
    }

    fn flat_lambda(&self, node: &SyntaxNode) -> Option<String> {
        let (params, body) = self.lambda_parts(node)?;
        let params = self.flat_lambda_params(params)?;
        let body = self.flat_lambda_body(body)?;
        Some(format!("{params} {body} }}"))
    }

    fn lambda_parts<'n>(&self, node: &'n SyntaxNode) -> Option<(&'n SyntaxNode, &'n SyntaxNode)> {
        let mut params: Option<&SyntaxNode> = None;
        let mut body: Option<&SyntaxNode> = None;
        for child in &node.children {
            if let Element::Node(inner) = child {
                if inner.kind == SyntaxKind::LambdaParams {
                    params = Some(inner);
                } else {
                    body = Some(inner);
                }
            }
        }
        Some((params?, body?))
    }

    fn flat_lambda_params(&self, node: &SyntaxNode) -> Option<String> {
        let mut names = Vec::new();
        for child in &node.children {
            match child {
                Element::Token(token) => match token.kind {
                    SyntaxKind::LBrace | SyntaxKind::Comma => {}
                    SyntaxKind::Arrow => break,
                    SyntaxKind::Whitespace | SyntaxKind::Eof => {}
                    SyntaxKind::Name | SyntaxKind::Underscore => {
                        names.push(self.token_text(token).to_string())
                    }
                    _ => return None,
                },
                Element::Node(_) => return None,
            }
        }
        if names.is_empty() {
            Some("{ ->".to_string())
        } else {
            Some(format!("{{ {} ->", names.join(", ")))
        }
    }

    fn flat_lambda_body(&self, node: &SyntaxNode) -> Option<String> {
        let mut inner: Vec<&SyntaxNode> = Vec::new();
        for child in &node.children {
            match child {
                Element::Token(token)
                    if token.kind == SyntaxKind::Whitespace
                        || token.kind == SyntaxKind::Eof
                        || token.kind == SyntaxKind::RBrace => {}
                Element::Token(_) => return None,
                Element::Node(inner_node) => inner.push(inner_node),
            }
        }
        match inner.as_slice() {
            [only] => self.flat(only),
            _ => None,
        }
    }

    fn flat_call(&self, node: &SyntaxNode) -> Option<String> {
        let elements = self.elements(node);
        let mut out = String::new();
        for (index, element) in elements.iter().enumerate() {
            match element {
                Element::Node(inner) if index == 0 => out.push_str(&self.flat(inner)?),
                Element::Node(inner) if inner.kind == SyntaxKind::ArgList => {
                    // The list touches the callee only when it is the whole
                    // argument list. `g (a, b) c` and `g(a, b) c` are different
                    // trees (`(g(a, b)) c` vs `g(a, b, c)`), so a list followed
                    // by more arguments keeps its space.
                    if index > 1 || elements.len() > 2 {
                        out.push(' ');
                    }
                    out.push_str(&self.flat(inner)?);
                }
                Element::Node(inner) => {
                    out.push(' ');
                    out.push_str(&self.flat(inner)?);
                }
                Element::Token(_) => return None,
            }
        }
        Some(out)
    }

    fn flat_index(&self, node: &SyntaxNode) -> Option<String> {
        let elements = self.elements(node);
        let mut out = String::new();
        for (index, element) in elements.iter().enumerate() {
            match element {
                Element::Node(inner) => out.push_str(&self.flat(inner)?),
                Element::Token(_) if index == 0 => return None,
                Element::Token(_) => return None,
            }
        }
        Some(out)
    }

    fn flat_array(&self, node: &SyntaxNode) -> Option<String> {
        let inner = self.first_node(node)?;
        if inner.kind == SyntaxKind::ArgList {
            self.flat_arg_list(inner)
        } else {
            self.flat_generic(node)
        }
    }

    fn flat_arg_list(&self, node: &SyntaxNode) -> Option<String> {
        let mut open = None;
        let mut close = None;
        let mut args = Vec::new();
        for child in &node.children {
            match child {
                Element::Token(token) => match token.kind {
                    SyntaxKind::LParen => open = Some("("),
                    SyntaxKind::RParen => close = Some(")"),
                    SyntaxKind::LBracket => open = Some("["),
                    SyntaxKind::RBracket => close = Some("]"),
                    SyntaxKind::Comma => {}
                    SyntaxKind::Whitespace | SyntaxKind::Eof => {}
                    _ => return None,
                },
                Element::Node(inner) => args.push(self.flat(inner)?),
            }
        }
        Some(format!("{}{}{}", open?, args.join(", "), close?))
    }

    fn flat_arg(&self, node: &SyntaxNode) -> Option<String> {
        let mut out = String::new();
        for child in &node.children {
            match child {
                Element::Token(token) => match token.kind {
                    SyntaxKind::KwLet
                    | SyntaxKind::KwInout
                    | SyntaxKind::KwSink
                    | SyntaxKind::KwSet => {
                        if !out.is_empty() {
                            out.push(' ');
                        }
                        out.push_str(self.token_text(token));
                    }
                    SyntaxKind::Whitespace | SyntaxKind::Eof => {}
                    _ => return None,
                },
                Element::Node(inner) => {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&self.flat(inner)?);
                }
            }
        }
        Some(out)
    }

    fn flat_binary(&self, node: &SyntaxNode) -> Option<String> {
        let elements = self.elements(node);
        let mut out = String::new();
        for element in elements {
            match element {
                Element::Node(inner) => out.push_str(&self.flat(inner)?),
                Element::Token(token) => {
                    let text = self.token_text(token);
                    if token.kind == SyntaxKind::Dot {
                        out.push('.');
                    } else if matches!(token.kind, SyntaxKind::DotDot | SyntaxKind::DotDotLt) {
                        out.push_str(text);
                    } else {
                        out.push(' ');
                        out.push_str(text);
                        out.push(' ');
                    }
                }
            }
        }
        Some(out)
    }

    fn flat_unary(&self, node: &SyntaxNode) -> Option<String> {
        let elements = self.elements(node);
        let mut out = String::new();
        let mut tight = false;
        for element in elements {
            match element {
                Element::Token(token) => {
                    if !tight && !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(self.token_text(token));
                    tight = matches!(
                        token.kind,
                        SyntaxKind::Minus | SyntaxKind::Star | SyntaxKind::Amp
                    );
                }
                Element::Node(inner) => {
                    if !tight && !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&self.flat(inner)?);
                    tight = false;
                }
            }
        }
        Some(out)
    }

    fn flat_type_ptr(&self, node: &SyntaxNode) -> Option<String> {
        let mut out = String::from("&raw");
        let mut pointee: Option<&SyntaxNode> = None;
        let mut saw_raw = false;
        for child in &node.children {
            match child {
                Element::Token(token) => match token.kind {
                    SyntaxKind::Amp => {}
                    SyntaxKind::Whitespace | SyntaxKind::Eof => {}
                    SyntaxKind::Name if !saw_raw && self.token_text(token) == "raw" => {
                        saw_raw = true
                    }
                    SyntaxKind::Name if self.token_text(token) == "mut" => out.push_str(" mut"),
                    _ => {}
                },
                Element::Node(inner) => pointee = Some(inner),
            }
        }
        out.push(' ');
        out.push_str(&self.flat(pointee?)?);
        Some(out)
    }

    fn flat_attribute(&self, node: &SyntaxNode) -> Option<String> {
        let mut out = String::from("@");
        let mut text = None;
        for child in &node.children {
            if let Element::Token(token) = child {
                match token.kind {
                    SyntaxKind::At | SyntaxKind::LParen | SyntaxKind::RParen => {}
                    SyntaxKind::Whitespace | SyntaxKind::Eof => {}
                    SyntaxKind::Name => out.push_str(self.token_text(token)),
                    SyntaxKind::Text => text = Some(self.token_text(token).to_string()),
                    _ => {}
                }
            }
        }
        out.push('(');
        out.push_str(&text?);
        out.push(')');
        if node.has_token(SyntaxKind::Semi) {
            out.push(';');
        }
        Some(out)
    }

    fn flat_include(&self, node: &SyntaxNode) -> Option<String> {
        let mut text = None;
        for child in &node.children {
            if let Element::Token(token) = child
                && matches!(token.kind, SyntaxKind::Text | SyntaxKind::RawText)
            {
                text = Some(self.token_text(token).to_string());
            }
        }
        Some(format!("@include({})", text?))
    }

    // -- node writer -------------------------------------------------------

    fn write_node(&mut self, node: &SyntaxNode, depth: usize) {
        use SyntaxKind::*;
        // A run of `///` lines lives inside the declaration it documents (the
        // parser attaches it), so print it before the declaration's own tokens.
        for child in &node.children {
            match child {
                Element::Token(token) if token.kind == Whitespace => {}
                Element::Token(token) if token.kind == DocComment => {
                    let text = self.token_text(token).trim_end().to_string();
                    self.emit(&text, depth);
                    self.newline();
                }
                _ => break,
            }
        }
        match node.kind {
            Root => self.root(node),
            Block => self.write_block(node, depth, true),
            IfExpr => self.write_if(node, depth),
            MatchExpr => self.write_match(node, depth),
            MatchArm => self.write_arm(node, depth),
            LambdaExpr => self.write_lambda(node, depth),
            CallExpr => self.write_call(node, depth),
            IndexExpr => self.write_index(node, depth),
            ArrayExpr => self.write_array(node, depth),
            TupleExpr => self.write_tuple(node, depth),
            ParenExpr => self.write_paren(node, depth),
            BinaryExpr => self.write_binary(node, depth),
            TryExpr => self.write_try(node, depth),
            Arg => self.write_arg(node, depth),
            ComptimeExpr | UnsafeExpr => self.write_comptime(node, depth),
            LetDecl | ProjectDecl => self.write_function_decl(node, depth),
            VarStmt => self.write_assign_like(node, depth, "var"),
            SetStmt => self.write_assign_like(node, depth, "set"),
            BindStmt => self.write_bind(node, depth),
            TryBindStmt => self.write_try_bind(node, depth),
            YieldStmt => self.write_yield(node, depth),
            ExprStmt => self.write_expr_stmt(node, depth),
            WhileStmt => self.write_loop(node, depth),
            ForStmt => self.write_for(node, depth),
            TypeDecl => self.write_type_decl(node, depth),
            TraitDecl => self.write_body_decl(node, depth, TraitBody, Layout::Members),
            ImplDecl => self.write_body_decl(node, depth, ImplBody, Layout::Members),
            BuilderDecl => self.write_body_decl(node, depth, BuilderBody, Layout::Members),
            ExternDecl => self.write_extern(node, depth),
            TraitBody | ImplBody | BuilderBody => {
                self.write_braced(node, depth, Layout::Members, false)
            }
            RecordDef | TypeView => self.write_braced(node, depth, Layout::Statements, true),
            _ => self.write_simple(node, depth),
        }
    }

    /// Nodes with no special layout: canonical one-liner, or a conservative
    /// token-by-token layout when a comment or a multi-line child is inside.
    fn write_simple(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node) {
            self.emit(&flat, depth);
            return;
        }
        self.write_loose(node, depth);
    }

    fn write_loose(&mut self, node: &SyntaxNode, depth: usize) {
        let mut prev: Option<(SyntaxKind, usize)> = None;
        for child in &node.children {
            match child {
                Element::Token(token)
                    if token.kind == SyntaxKind::Whitespace || token.kind == SyntaxKind::Eof => {}
                Element::Token(token) if token.kind == SyntaxKind::DocComment => {}
                Element::Token(token) if token.kind.is_trivia() => {
                    let start = token.span.start.to_usize();
                    let trailing = prev.is_some_and(|(_, end)| self.source_same_line(end, start));
                    let text = self.token_text(token).trim_end().to_string();
                    if trailing && !self.at_line_start() {
                        self.out.push(' ');
                        self.out.push_str(&text);
                        self.newline();
                    } else {
                        if !self.at_line_start() {
                            self.newline();
                        }
                        self.emit(&text, depth);
                        self.newline();
                    }
                    prev = Some((token.kind, token.span.end.to_usize()));
                }
                Element::Token(token) => {
                    if let Some((previous, _)) = prev
                        && join_space(previous, token.kind)
                        && !self.at_line_start()
                    {
                        self.out.push(' ');
                    }
                    let text = self.token_text(token).to_string();
                    self.emit(&text, depth);
                    prev = Some((token.kind, token.span.end.to_usize()));
                }
                Element::Node(inner) => {
                    let first = self.first_kind(inner);
                    if let (Some((previous, _)), Some(first)) = (prev, first)
                        && join_space(previous, first)
                        && !self.at_line_start()
                    {
                        self.out.push(' ');
                    }
                    self.write_node(inner, depth);
                    prev = Some((
                        self.last_kind(inner).unwrap_or(SyntaxKind::Error),
                        inner.span.end.to_usize(),
                    ));
                }
            }
        }
    }

    /// Writes a value after `= ` / `:= `. A block decides between its one-line
    /// and multi-line form; an `if` chain that does not fit the line but does
    /// fit the next one is indented under the `=`, the way the examples write a
    /// long condition; everything else breaks internally (arguments, operators)
    /// at its current position.
    fn write_value(&mut self, node: &SyntaxNode, depth: usize) {
        match node.kind {
            SyntaxKind::Block => self.write_block(node, depth, true),
            SyntaxKind::IfExpr => self.write_if_value(node, depth),
            _ => {
                if let Some(flat) = self.flat(node) {
                    if self.fits(&flat, depth, self.cfg.max_width) {
                        self.emit(&flat, depth);
                        return;
                    }
                    // The header used up this line but the value alone fits the
                    // next one: indent it under the `=` instead of splitting it.
                    let own = flat.chars().count() + (depth + 1) * self.cfg.tab_width;
                    if own <= self.cfg.max_width && !self.at_line_start() {
                        self.newline();
                        self.emit(&flat, depth + 1);
                        return;
                    }
                }
                self.write_node(node, depth);
            }
        }
    }

    /// An `if` value: inline when the chain fits the `if` budget on this line,
    /// indented onto the next line when it fits there, broken per block
    /// otherwise.
    fn write_if_value(&mut self, node: &SyntaxNode, depth: usize) {
        let budget = self.cfg.single_line_if_width.min(self.cfg.max_width);
        if let Some(flat) = self.flat(node) {
            if self.fits(&flat, depth, budget) {
                self.emit(&flat, depth);
                return;
            }
            let own = flat.chars().count() + (depth + 1) * self.cfg.tab_width;
            if own <= budget && !self.at_line_start() {
                self.newline();
                self.emit(&flat, depth + 1);
                return;
            }
        }
        self.write_if(node, depth);
    }

    // -- blocks ------------------------------------------------------------

    fn write_block(&mut self, node: &SyntaxNode, depth: usize, allow_inline: bool) {
        if allow_inline
            && let Some(flat) = self.flat_block(node)
            && flat.chars().count() <= self.cfg.single_line_block_width
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        self.write_braced(node, depth, Layout::Statements, true);
    }

    fn write_braced(
        &mut self,
        node: &SyntaxNode,
        depth: usize,
        layout: Layout,
        allow_inline: bool,
    ) {
        if allow_inline
            && let Some(flat) = self.flat_braced(node)
            && flat.chars().count() <= self.cfg.single_line_block_width
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        self.emit("{", depth);
        let empty = !node.children.iter().any(|child| match child {
            Element::Token(token) => {
                !token.kind.is_trivia()
                    && token.kind != SyntaxKind::LBrace
                    && token.kind != SyntaxKind::RBrace
                    && token.kind != SyntaxKind::Eof
            }
            Element::Node(_) => true,
        });
        if empty {
            self.emit("}", depth);
            return;
        }
        self.newline();
        self.write_items(&node.children, depth + 1, layout);
        self.newline();
        self.emit("}", depth);
    }

    /// One-line form of a braced body holding a single item.
    fn flat_braced(&self, node: &SyntaxNode) -> Option<String> {
        let inner: Vec<&SyntaxNode> = node.children.iter().filter_map(Element::as_node).collect();
        match inner.as_slice() {
            [] => Some("{}".to_string()),
            [only] => Some(format!("{{ {} }}", self.flat(only)?)),
            _ => None,
        }
    }

    // -- expressions -------------------------------------------------------

    fn write_if(&mut self, node: &SyntaxNode, depth: usize) {
        let budget = self.cfg.single_line_if_width.min(self.cfg.max_width);
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, budget)
        {
            self.emit(&flat, depth);
            return;
        }
        let Some((condition, then_block, else_branch)) = self.if_parts(node) else {
            self.write_loose(node, depth);
            return;
        };
        self.emit("if (", depth);
        self.write_node(condition, depth);
        self.emit(") ", depth);
        self.write_braced(then_block, depth, Layout::Statements, false);
        if let Some(branch) = else_branch {
            self.emit(" else ", depth);
            match branch.kind {
                SyntaxKind::IfExpr => self.write_if(branch, depth),
                SyntaxKind::Block => self.write_braced(branch, depth, Layout::Statements, false),
                _ => self.write_node(branch, depth),
            }
        }
    }

    fn write_match(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let (scrutinee, arms) = self.match_parts(node);
        let Some(scrutinee) = scrutinee else {
            self.write_loose(node, depth);
            return;
        };
        self.emit("match ", depth);
        self.write_node(scrutinee, depth);
        self.emit(" {", depth);
        self.newline();
        for (index, arm) in arms.iter().enumerate() {
            self.write_arm(arm, depth + 1);
            if index + 1 < arms.len() {
                self.emit(",", depth + 1);
            }
            self.newline();
        }
        self.emit("}", depth);
    }

    fn write_arm(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let (pattern, guard, body) = self.arm_parts(node);
        if let Some(pattern) = pattern
            && let Some(flat) = self.flat(pattern)
        {
            self.emit(&flat, depth);
        }
        if let Some(guard) = guard
            && let Some(flat) = self.flat(guard)
        {
            self.emit(" if ", depth);
            self.emit(&flat, depth);
        }
        self.emit(" ->", depth);
        if let Some(body) = body {
            match self.flat(body) {
                Some(flat) if self.fits(&flat, depth, self.cfg.max_width) => {
                    self.emit(" ", depth);
                    self.emit(&flat, depth);
                }
                _ => {
                    self.newline();
                    self.write_node(body, depth + 1);
                }
            }
        }
    }

    fn write_lambda(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let Some((params, body)) = self.lambda_parts(node) else {
            self.write_loose(node, depth);
            return;
        };
        if let Some(flat) = self.flat_lambda_params(params) {
            self.emit(&flat, depth);
        }
        self.newline();
        self.write_items(&body.children, depth + 1, Layout::Statements);
        self.newline();
        self.emit("}", depth);
    }

    fn write_call(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let elements = self.elements(node);
        for (index, element) in elements.iter().enumerate() {
            match element {
                Element::Node(inner) if index == 0 => self.write_node(inner, depth),
                Element::Node(inner) if inner.kind == SyntaxKind::ArgList => {
                    // Only the sole parenthesised argument list may touch the
                    // callee; `f (x) (y)` is one flat call with two lists while
                    // `f(x) (y)` is a call of the result, so the space stays.
                    let adjacent = index == 1 && elements.len() == 2;
                    if !adjacent {
                        self.emit(" ", depth);
                    }
                    if adjacent {
                        self.emit("(", depth);
                        self.write_arguments(inner, depth);
                    } else if let Some(flat) = self.flat(inner) {
                        self.emit(&flat, depth);
                    } else {
                        self.write_loose(inner, depth);
                    }
                }
                Element::Node(inner) => {
                    self.emit(" ", depth);
                    self.write_node(inner, depth);
                }
                Element::Token(_) => {}
            }
        }
    }

    /// Arguments of a call or elements of an array, one per line when the list
    /// does not fit.
    fn write_arguments(&mut self, arg_list: &SyntaxNode, depth: usize) {
        let close = if arg_list.has_token(SyntaxKind::RBracket) {
            "]"
        } else {
            ")"
        };
        if let Some(flat) = self.flat_arg_list(arg_list)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let elements = self.elements(arg_list);
        let args: Vec<&SyntaxNode> = elements
            .iter()
            .filter_map(|element| element.as_node())
            .collect();
        if args.is_empty() {
            self.emit(close, depth);
            return;
        }
        let mut arg_index = 0usize;
        for element in &elements {
            match element {
                Element::Token(token) if token.kind.is_trivia() => {
                    let text = self.token_text(token).trim_end().to_string();
                    self.newline();
                    self.emit(&text, depth + 1);
                    self.newline();
                }
                Element::Token(_) => {}
                Element::Node(arg) => {
                    self.newline();
                    self.write_node(arg, depth + 1);
                    if arg_index + 1 < args.len() {
                        self.emit(",", depth + 1);
                    }
                    arg_index += 1;
                }
            }
        }
        self.newline();
        self.emit(close, depth);
    }

    fn write_arg(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        for child in &node.children {
            match child {
                Element::Token(token)
                    if matches!(
                        token.kind,
                        SyntaxKind::KwLet
                            | SyntaxKind::KwInout
                            | SyntaxKind::KwSink
                            | SyntaxKind::KwSet
                    ) =>
                {
                    let text = self.token_text(token).to_string();
                    self.emit(&text, depth);
                    self.emit(" ", depth);
                }
                Element::Token(token)
                    if token.kind == SyntaxKind::Whitespace || token.kind == SyntaxKind::Eof => {}
                Element::Token(_) => {}
                Element::Node(inner) => self.write_node(inner, depth),
            }
        }
    }

    fn write_index(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let elements = self.elements(node);
        let mut base: Option<&SyntaxNode> = None;
        let mut arg_list: Option<&SyntaxNode> = None;
        for (index, element) in elements.iter().enumerate() {
            if let Element::Node(inner) = element {
                if index == 0 {
                    base = Some(inner);
                } else if inner.kind == SyntaxKind::ArgList {
                    arg_list = Some(inner);
                }
            }
        }
        match (base, arg_list) {
            (Some(base), Some(arg_list)) => {
                self.write_node(base, depth);
                self.emit("[", depth);
                self.write_arguments(arg_list, depth);
            }
            _ => self.write_loose(node, depth),
        }
    }

    fn write_array(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let arg_list = node
            .children
            .iter()
            .filter_map(Element::as_node)
            .find(|inner| inner.kind == SyntaxKind::ArgList);
        match arg_list {
            Some(arg_list) => {
                self.emit("[", depth);
                self.write_arguments(arg_list, depth);
            }
            None => self.write_loose(node, depth),
        }
    }

    fn write_tuple(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let parts = self.node_parts(node);
        if parts.is_empty() {
            self.emit("()", depth);
            return;
        }
        self.emit("(", depth);
        for (index, part) in parts.iter().enumerate() {
            self.newline();
            self.write_node(part, depth + 1);
            if index + 1 < parts.len() {
                self.emit(",", depth + 1);
            }
        }
        self.newline();
        self.emit(")", depth);
    }

    fn write_paren(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        self.emit("(", depth);
        if let Some(inner) = self.first_node(node) {
            self.write_node(inner, depth);
        }
        self.emit(")", depth);
    }

    fn write_try(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        if let Some(base) = self.first_node(node) {
            self.write_node(base, depth);
        }
        self.emit("?", depth);
    }

    fn write_binary(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let mut chain: Vec<(Option<String>, &SyntaxNode)> = Vec::new();
        self.collect_chain(node, &mut chain);
        // A multi-line operand (a handler block, a match) owns its layout: the
        // operator stays on the line that closes it rather than starting a new
        // one, which is what keeps `(option { ... })? | 0` readable.
        let splittable = chain.len() > 1
            && chain
                .iter()
                .all(|(_, operand)| self.flat(operand).is_some());
        if !splittable {
            self.write_binary_inline(node, depth);
            return;
        }
        for (operator, operand) in chain {
            match operator {
                None => self.write_node(operand, depth),
                Some(operator) => {
                    self.newline();
                    self.emit(&operator, depth + 1);
                    self.emit(" ", depth + 1);
                    self.write_node(operand, depth + 1);
                }
            }
        }
    }

    /// Binary layout with no breaks: left, operator, right, letting each
    /// operand lay itself out.
    fn write_binary_inline(&mut self, node: &SyntaxNode, depth: usize) {
        for child in &node.children {
            match child {
                Element::Node(inner) => self.write_node(inner, depth),
                Element::Token(token) if token.kind == SyntaxKind::Dot => self.emit(".", depth),
                Element::Token(token) if token.kind.is_trivia() => {}
                Element::Token(token) => {
                    let text = self.token_text(token).to_string();
                    self.emit(" ", depth);
                    self.emit(&text, depth);
                    self.emit(" ", depth);
                }
            }
        }
    }

    /// Flattens a chain of the same binary operator so a long expression breaks
    /// one operand per line instead of one operator per level.
    fn collect_chain<'n>(
        &self,
        node: &'n SyntaxNode,
        out: &mut Vec<(Option<String>, &'n SyntaxNode)>,
    ) {
        let elements = self.elements(node);
        if elements.len() == 3
            && let (Element::Node(left), Element::Token(operator), Element::Node(right)) =
                (&elements[0], &elements[1], &elements[2])
            && operator.kind != SyntaxKind::Dot
        {
            let text = self.token_text(operator).to_string();
            if left.kind == SyntaxKind::BinaryExpr
                && self.operator_text(left).as_deref() == Some(&text)
            {
                self.collect_chain(left, out);
                out.push((Some(text), right));
                return;
            }
            if right.kind == SyntaxKind::BinaryExpr
                && self.operator_text(right).as_deref() == Some(&text)
            {
                out.push((None, left));
                let mut rest = Vec::new();
                self.collect_chain(right, &mut rest);
                if let Some((None, first)) = rest.first().cloned() {
                    out.push((Some(text), first));
                    out.extend(rest.into_iter().skip(1));
                    return;
                }
            }
            out.push((None, left));
            out.push((Some(text), right));
            return;
        }
        out.push((None, node));
    }

    fn operator_text(&self, node: &SyntaxNode) -> Option<String> {
        node.children.iter().find_map(|child| match child {
            Element::Token(token) if !token.kind.is_trivia() => {
                Some(self.token_text(token).to_string())
            }
            _ => None,
        })
    }

    fn write_comptime(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let keyword = if node.kind == SyntaxKind::ComptimeExpr {
            "comptime "
        } else {
            "unsafe "
        };
        self.emit(keyword, depth);
        if let Some(block) = self.first_node(node) {
            self.write_block(block, depth, true);
        }
    }

    // -- statements --------------------------------------------------------

    fn write_expr_stmt(&mut self, node: &SyntaxNode, depth: usize) {
        let Some(expr) = self.first_node(node) else {
            self.write_loose(node, depth);
            return;
        };
        self.write_node(expr, depth);
        if node.has_token(SyntaxKind::Semi) {
            self.emit(";", depth);
        }
    }

    fn has_semi(&self, elements: &[&Element]) -> bool {
        elements.iter().any(
            |element| matches!(element, Element::Token(token) if token.kind == SyntaxKind::Semi),
        )
    }

    /// `let` / `project` declarations: header, `=`, body, `;`.
    fn write_function_decl(&mut self, node: &SyntaxNode, depth: usize) {
        let elements = self.elements(node);
        let Some(assign_index) = elements.iter().position(
            |element| matches!(element, Element::Token(token) if token.kind == SyntaxKind::Assign),
        ) else {
            self.write_simple(node, depth);
            return;
        };
        self.write_header(&elements[..assign_index], depth);
        self.emit(" = ", depth);
        let body = elements[assign_index + 1..]
            .iter()
            .find_map(|element| element.as_node());
        if let Some(body) = body {
            self.write_value(body, depth);
        }
        if self.has_semi(&elements) {
            self.emit(";", depth);
        }
    }

    /// Writes a declaration header, breaking a parameter list one per line
    /// when the flat form would not fit.
    fn write_header(&mut self, elements: &[&Element], depth: usize) {
        if let Some(flat) = self.flat_elements(elements)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let param_list = elements
            .iter()
            .position(|element| matches!(element, Element::Node(inner) if inner.kind == SyntaxKind::ParamList));
        let Some(index) = param_list else {
            if let Some(flat) = self.flat_elements(elements) {
                self.emit(&flat, depth);
            }
            return;
        };
        if let Some(flat) = self.flat_elements(&elements[..index]) {
            self.emit(&flat, depth);
        }
        let Element::Node(params) = elements[index] else {
            return;
        };
        let params_inner: Vec<&SyntaxNode> = params
            .children
            .iter()
            .filter_map(Element::as_node)
            .filter(|inner| inner.kind == SyntaxKind::Param)
            .collect();
        if params_inner.len() <= 1 {
            if let Some(flat) = self.flat(params) {
                self.emit(&flat, depth);
            }
        } else {
            self.emit("(", depth);
            for (param_index, param) in params_inner.iter().enumerate() {
                self.newline();
                self.write_node(param, depth + 1);
                if param_index + 1 < params_inner.len() {
                    self.emit(",", depth + 1);
                }
            }
            self.newline();
            self.emit(")", depth);
        }
        if let Some(flat) = self.flat_elements(&elements[index + 1..]) {
            self.emit(" ", depth);
            self.emit(&flat, depth);
        }
    }

    /// `var name: T = expr;` and `set place = expr;`.
    fn write_assign_like(&mut self, node: &SyntaxNode, depth: usize, keyword: &str) {
        let elements = self.elements(node);
        let Some(assign_index) = elements.iter().position(
            |element| matches!(element, Element::Token(token) if token.kind == SyntaxKind::Assign),
        ) else {
            self.write_simple(node, depth);
            return;
        };
        if let Some(header) = self.flat_elements(&elements[..assign_index]) {
            self.emit(&header, depth);
            self.emit(" = ", depth);
        } else {
            self.emit(keyword, depth);
        }
        if let Some(body) = elements[assign_index + 1..]
            .iter()
            .find_map(|element| element.as_node())
        {
            self.write_value(body, depth);
        }
        if self.has_semi(&elements) {
            self.emit(";", depth);
        }
    }

    fn write_bind(&mut self, node: &SyntaxNode, depth: usize) {
        let elements = self.elements(node);
        let Some(assign_index) = elements.iter().position(
            |element| matches!(element, Element::Token(token) if token.kind == SyntaxKind::ColonEq),
        ) else {
            self.write_simple(node, depth);
            return;
        };
        if let Some(header) = self.flat_elements(&elements[..assign_index]) {
            self.emit(&header, depth);
            self.emit(" := ", depth);
        }
        if let Some(body) = elements[assign_index + 1..]
            .iter()
            .find_map(|element| element.as_node())
        {
            self.write_value(body, depth);
        }
        if self.has_semi(&elements) {
            self.emit(";", depth);
        }
    }

    fn write_try_bind(&mut self, node: &SyntaxNode, depth: usize) {
        let elements = self.elements(node);
        let Some(assign_index) = elements.iter().position(
            |element| matches!(element, Element::Token(token) if token.kind == SyntaxKind::Assign),
        ) else {
            self.write_simple(node, depth);
            return;
        };
        if let Some(header) = self.flat_elements(&elements[..assign_index]) {
            self.emit(&header, depth);
            self.emit(" = ", depth);
        }
        if let Some(body) = elements[assign_index + 1..]
            .iter()
            .find_map(|element| element.as_node())
        {
            self.write_value(body, depth);
        }
        if self.has_semi(&elements) {
            self.emit(";", depth);
        }
    }

    fn write_yield(&mut self, node: &SyntaxNode, depth: usize) {
        let elements = self.elements(node);
        let inout = elements.iter().any(
            |element| matches!(element, Element::Token(token) if token.kind == SyntaxKind::KwInout),
        );
        self.emit("yield ", depth);
        if inout {
            self.emit("inout ", depth);
        }
        if let Some(body) = elements.iter().find_map(|element| element.as_node()) {
            self.write_value(body, depth);
        }
        if self.has_semi(&elements) {
            self.emit(";", depth);
        }
    }

    fn write_loop(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let mut condition: Option<&SyntaxNode> = None;
        let mut body: Option<&SyntaxNode> = None;
        for child in &node.children {
            if let Element::Node(inner) = child {
                if condition.is_none() {
                    condition = Some(inner);
                } else {
                    body = Some(inner);
                }
            }
        }
        self.emit("while (", depth);
        if let Some(condition) = condition {
            self.write_node(condition, depth);
        }
        self.emit(") ", depth);
        if let Some(body) = body {
            if body.kind == SyntaxKind::Block {
                self.write_block(body, depth, true);
            } else {
                self.write_node(body, depth);
            }
        }
    }

    fn write_for(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let mut bind: Option<&SyntaxNode> = None;
        let mut body: Option<&SyntaxNode> = None;
        for child in &node.children {
            if let Element::Node(inner) = child {
                if inner.kind == SyntaxKind::BindStmt && bind.is_none() {
                    bind = Some(inner);
                } else {
                    body = Some(inner);
                }
            }
        }
        self.emit("for (", depth);
        if let Some(bind) = bind
            && let Some(flat) = self.flat(bind)
        {
            self.emit(&flat, depth);
        }
        self.emit(") ", depth);
        if let Some(body) = body {
            if body.kind == SyntaxKind::Block {
                self.write_block(body, depth, true);
            } else {
                self.write_node(body, depth);
            }
        }
    }

    // -- declarations ------------------------------------------------------

    fn write_type_decl(&mut self, node: &SyntaxNode, depth: usize) {
        let elements = self.elements(node);
        let Some(assign_index) = elements.iter().position(
            |element| matches!(element, Element::Token(token) if token.kind == SyntaxKind::Assign),
        ) else {
            self.write_simple(node, depth);
            return;
        };
        if let Some(header) = self.flat_elements(&elements[..assign_index]) {
            self.emit(&header, depth);
            self.emit(" = ", depth);
        }
        if let Some(body) = elements[assign_index + 1..]
            .iter()
            .find_map(|element| element.as_node())
        {
            match body.kind {
                SyntaxKind::RecordDef => {
                    self.emit("record ", depth);
                    self.write_braced(body, depth, Layout::Statements, true);
                }
                SyntaxKind::TypeView => {
                    self.emit("view ", depth);
                    self.write_braced(body, depth, Layout::Statements, true);
                }
                _ => self.write_value(body, depth),
            }
        }
        if self.has_semi(&elements) {
            self.emit(";", depth);
        }
    }

    fn write_body_decl(
        &mut self,
        node: &SyntaxNode,
        depth: usize,
        body_kind: SyntaxKind,
        layout: Layout,
    ) {
        let elements = self.elements(node);
        let body_index = elements
            .iter()
            .position(|element| matches!(element, Element::Node(inner) if inner.kind == body_kind));
        let Some(body_index) = body_index else {
            self.write_simple(node, depth);
            return;
        };
        if let Some(header) = self.flat_elements(&elements[..body_index]) {
            self.emit(&header, depth);
            self.emit(" ", depth);
        }
        if let Some(Element::Node(body)) = elements.get(body_index) {
            self.write_braced(body, depth, layout, false);
        }
        if self.has_semi(&elements) {
            self.emit(";", depth);
        }
    }

    fn write_extern(&mut self, node: &SyntaxNode, depth: usize) {
        if let Some(flat) = self.flat(node)
            && self.fits(&flat, depth, self.cfg.max_width)
        {
            self.emit(&flat, depth);
            return;
        }
        let elements = self.elements(node);
        let mut out = String::from("extern");
        for element in &elements {
            if let Element::Token(token) = element
                && token.kind == SyntaxKind::Text
            {
                out.push(' ');
                out.push_str(self.token_text(token));
            }
        }
        out.push(' ');
        self.emit(&out, depth);
        if let Some(inner) = elements.iter().find_map(|element| element.as_node()) {
            self.write_node(inner, depth);
        }
    }
}

/// Whether a space belongs between two adjacent constructs in the generic
/// layout. Bracket adjacency is structural and never reaches this function.
fn join_space(prev: SyntaxKind, next: SyntaxKind) -> bool {
    use SyntaxKind::*;
    if matches!(
        next,
        Comma | Semi | RParen | RBracket | Colon | Dot | ColonColon | Question
    ) {
        return false;
    }
    if matches!(prev, LParen | LBracket | Dot | ColonColon | At) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{Config, format_text, format_text_with};

    #[test]
    fn static_var_keeps_its_storage_qualifiers() {
        let src = "static var pool: i32 = 1;\nstatic threadlocal var scratch: i32 = 2;\nlet main () -> i32 <computes> = pool + scratch;\n";
        let out = format_text(src);
        assert!(out.contains("static var pool: i32 = 1;"), "{out}");
        assert!(
            out.contains("static threadlocal var scratch: i32 = 2;"),
            "{out}"
        );
        assert_eq!(format_text(&out), out, "formatting is idempotent");
    }

    #[test]
    fn call_parens_touch_the_callee() {
        let src =
            "let main () -> i32 <io> = {\n    var x = add (1, 2);\n    io::println (\"x\")\n};\n";
        let out = format_text(src);
        assert!(out.contains("add(1, 2)"), "{out}");
        assert!(out.contains("io::println(\"x\")"), "{out}");
        assert!(!out.contains("add (1, 2)"), "{out}");
    }

    #[test]
    fn a_second_argument_list_keeps_its_space() {
        let src = "let main () -> i32 <io> = {\n    g (a) (b)\n};\n";
        let out = format_text(src);
        assert!(out.contains("g (a) (b)"), "{out}");
    }

    #[test]
    fn adjacent_brackets_stay_adjacent_and_spaced_brackets_stay_spaced() {
        let src =
            "let main () -> i32 <io> = {\n    var a = f[x];\n    var b = f [x];\n    a + b\n};\n";
        let out = format_text(src);
        assert!(out.contains("f[x]"), "{out}");
        assert!(out.contains("f [x]"), "{out}");
    }

    #[test]
    fn a_spliced_list_followed_by_arguments_keeps_its_space() {
        let src = "let main () -> i32 <io> = {\n    g (a, b) c\n};\n";
        let out = format_text(src);
        assert!(out.contains("g (a, b) c"), "{out}");
    }

    #[test]
    fn short_if_stays_on_one_line() {
        let src = "let f (x: Bool) -> i32 <computes> = if (x) { 0 } else { 1 };\n";
        assert_eq!(format_text(src), src);
    }

    #[test]
    fn long_if_breaks_over_lines() {
        let src = "let f (x: i32, y: i32) -> i32 <computes> = if (x > 0) { x + y + 1 } else { x + y + 2 };\n";
        let out = format_text(src);
        assert!(out.contains("if (x > 0) {"), "{out}");
        assert!(out.contains("} else {"), "{out}");
    }

    #[test]
    fn blank_line_between_functions() {
        let src = "let a () -> i32 <computes> = 1;\nlet b () -> i32 <computes> = 2;\n";
        let out = format_text(src);
        assert!(out.contains("= 1;\n\nlet b"), "{out}");
    }

    #[test]
    fn blank_line_between_impl_members() {
        let src = "impl Show Point = {\n    let a () -> i32 <computes> = 1;\n    let b () -> i32 <computes> = 2;\n};\n";
        let out = format_text(src);
        assert!(out.contains("= 1;\n\n\tlet b"), "{out}");
    }

    #[test]
    fn config_changes_the_if_threshold() {
        let config = Config {
            single_line_if_width: 200,
            ..Config::default()
        };
        let src = "let f (x: i32, y: i32) -> i32 <computes> = if (x > 0) { x + y + 1 } else { x + y + 2 };\n";
        let out = format_text_with(src, &config);
        assert!(
            out.contains("if (x > 0) { x + y + 1 } else { x + y + 2 }"),
            "{out}"
        );
        let narrow = format_text(src);
        assert!(narrow.contains("} else {"), "{narrow}");
    }

    #[test]
    fn config_changes_the_block_threshold() {
        let config = Config {
            single_line_block_width: 200,
            single_line_if_width: 200,
            ..Config::default()
        };
        let src = "let f (x: i32) -> i32 <computes> = if (x > 0) { x + 1 } else { x - 1 };\n";
        assert_eq!(format_text_with(src, &config), src);
    }

    #[test]
    fn long_argument_lists_break_one_per_line() {
        let config = Config {
            max_width: 30,
            ..Config::default()
        };
        let src = "let main () -> i32 <io> = {\n    var x = call (11111, 22222, 33333, 44444);\n    x\n};\n";
        let out = format_text_with(src, &config);
        assert!(out.contains("call(\n"), "{out}");
        assert!(out.contains("\t\t11111,"), "{out}");
        assert!(out.contains("\t\t44444\n"), "{out}");
    }

    #[test]
    fn a_value_that_fits_the_next_line_is_indented_under_the_equals() {
        let config = Config {
            max_width: 40,
            ..Config::default()
        };
        let src = "let main () -> i32 <io> = {\n    var x = call (11111, 22222, 33333, 44444);\n    x\n};\n";
        let out = format_text_with(src, &config);
        assert!(
            out.contains("var x =\n\t\tcall(11111, 22222, 33333, 44444);"),
            "{out}"
        );
    }

    #[test]
    fn long_parameter_lists_break_one_per_line() {
        let config = Config {
            max_width: 40,
            ..Config::default()
        };
        let src = "let f (alpha: i32, beta: i32, gamma: i32) -> i32 <computes> = 0;\n";
        let out = format_text_with(src, &config);
        assert!(out.contains("let f(\n"), "{out}");
        assert!(out.contains("\tbeta: i32,"), "{out}");
    }

    #[test]
    fn empty_parens_and_empty_lambda_params_are_tight() {
        let src = "let main () -> i32 <io> = {\n    var t = spawn_thread ({ -> () });\n    0\n};\n";
        let out = format_text(src);
        assert!(out.contains("spawn_thread({ -> () })"), "{out}");
        assert!(!out.contains("{  ->"), "{out}");
    }

    #[test]
    fn doc_comments_keep_their_place() {
        let src = "/// Doubles a number.\n///\n/// Second paragraph.\nlet double (x: i32) -> i32 <computes> = x * 2;\n";
        let once = format_text(src);
        assert_eq!(once, src);
        assert_eq!(format_text(&once), once);
    }

    #[test]
    fn unit_values_keep_their_parens() {
        let src = "let f () -> () <computes> = ();\n";
        assert_eq!(format_text(src), src);
    }

    #[test]
    fn formatting_is_idempotent() {
        let src = "let f (x: i32) -> i32 <computes> = if (x > 0) { x } else { 0 };\n\nlet main () -> i32 <io> = {\n    var y = f (1);\n    io::println (\"y\");\n    y\n};\n";
        let once = format_text(src);
        assert_eq!(format_text(&once), once);
    }
}
