//! Compiler diagnostics: accumulation, and ariadne-style terminal rendering
//! with primary/secondary labels and notes.

use std::fmt::Write as _;

use rill_span::{SourceMap, Span};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Label {
    pub span: Span,
    pub message: String,
    pub primary: bool,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn new(severity: Severity, span: Span, message: impl Into<String>) -> Self {
        let labels = if span.is_dummy() {
            Vec::new()
        } else {
            vec![Label {
                span,
                message: String::new(),
                primary: true,
            }]
        };
        Diagnostic {
            severity,
            message: message.into(),
            labels,
            notes: Vec::new(),
        }
    }

    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Error, span, message)
    }

    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Warning, span, message)
    }

    /// Adds a labeled span. `primary` labels are rendered with `^^^`.
    pub fn with_label(mut self, span: Span, message: impl Into<String>, primary: bool) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary,
        });
        self
    }

    pub fn primary_label(self, span: Span, message: impl Into<String>) -> Self {
        self.with_label(span, message, true)
    }

    pub fn secondary_label(self, span: Span, message: impl Into<String>) -> Self {
        self.with_label(span, message, false)
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// The span rendered as the diagnostic's location (first primary label,
    /// else first label, else dummy).
    pub fn location_span(&self) -> Span {
        self.labels
            .iter()
            .find(|l| l.primary)
            .or_else(|| self.labels.first())
            .map(|l| l.span)
            .unwrap_or(Span::DUMMY)
    }
}

/// Accumulates diagnostics during a compilation phase.
#[derive(Default, Clone, Debug)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    errors: usize,
    warnings: usize,
}

impl Diagnostics {
    pub fn new() -> Self {
        Diagnostics::default()
    }

    pub fn push(&mut self, diag: Diagnostic) {
        match diag.severity {
            Severity::Error => self.errors += 1,
            Severity::Warning => self.warnings += 1,
            _ => {}
        }
        self.items.push(diag);
    }

    /// Convenience short-hand: pushes an error diagnostic.
    pub fn error(&mut self, span: Span, message: impl Into<String>) {
        self.push(Diagnostic::error(span, message));
    }

    /// Convenience short-hand: pushes a warning diagnostic.
    pub fn warning(&mut self, span: Span, message: impl Into<String>) {
        self.push(Diagnostic::warning(span, message));
    }

    pub fn extend(&mut self, other: Diagnostics) {
        for diag in other.items {
            self.push(diag);
        }
    }

    pub fn has_errors(&self) -> bool {
        self.errors > 0
    }

    pub fn error_count(&self) -> usize {
        self.errors
    }

    pub fn warning_count(&self) -> usize {
        self.warnings
    }

    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn into_items(self) -> Vec<Diagnostic> {
        self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Renders all diagnostics to a plain-text report (no ANSI colors).
pub fn render(source_map: &SourceMap, diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for (i, diag) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        render_one(source_map, diag, &mut out);
    }
    out
}

/// Renders a diagnostic set, suitable for `eprint!`.
pub fn render_all(source_map: &SourceMap, diagnostics: &Diagnostics) -> String {
    render(source_map, diagnostics.items())
}

fn render_one(source_map: &SourceMap, diag: &Diagnostic, out: &mut String) {
    let _ = writeln!(out, "{}: {}", diag.severity.as_str(), diag.message);

    let mut labels: Vec<&Label> = diag.labels.iter().collect();
    labels.sort_by_key(|l| l.span.start);

    // Gutter width: line numbers of all rendered labels.
    let max_line = labels
        .iter()
        .filter(|l| !l.span.is_dummy())
        .map(|l| source_map.line_col(l.span.start).line)
        .max()
        .unwrap_or(0);
    let gutter = max_line.to_string().len().max(1);

    let has_location = diag.location_span() != Span::DUMMY;
    if has_location {
        let mut first = true;
        for label in &labels {
            if label.span.is_dummy() {
                continue;
            }
            if first {
                let _ = writeln!(
                    out,
                    "{:width$}--> {}",
                    "",
                    source_map.location(label.span),
                    width = gutter + 1
                );
                first = false;
            }
            render_label(source_map, label, gutter, out);
        }
    } else if !labels.is_empty() {
        for label in &labels {
            if !label.message.is_empty() {
                let _ = writeln!(out, "  = {}", label.message);
            }
        }
    }

    for note in &diag.notes {
        let _ = writeln!(out, "{:width$} = note: {}", "", note, width = gutter + 1);
    }
}

fn render_label(source_map: &SourceMap, label: &Label, gutter: usize, out: &mut String) {
    if label.span.is_dummy() {
        if !label.message.is_empty() {
            let _ = writeln!(out, "  = {}", label.message);
        }
        return;
    }

    let lc = source_map.line_col(label.span.start);
    let line_text = source_map.line_text(label.span.start);
    let end_lc = source_map.line_col(label.span.end);
    let end_line_text = source_map.line_text(label.span.end);
    let same_line = lc.line == end_lc.line;

    let pad = " ".repeat(gutter);
    let _ = writeln!(out, "{pad} |");

    // Source line(s): render the first and, when different, the last line.
    let _ = writeln!(out, "{:>width$} | {}", lc.line, line_text, width = gutter);
    if !same_line && line_text != end_line_text && end_lc.line != 0 {
        let _ = writeln!(out, "{pad} | ...");
        let _ = writeln!(
            out,
            "{:>width$} | {}",
            end_lc.line,
            end_line_text,
            width = gutter
        );
    }

    // Caret under the span (clamped to the underlying line).
    let caret_start = (lc.col as usize).saturating_sub(1);
    let caret_len = if same_line {
        let len = (label.span.len().max(1))
            .min(line_text.chars().count().saturating_sub(caret_start).max(1));
        len
    } else {
        line_text.chars().count().saturating_sub(caret_start).max(1)
    };
    let marker = if label.primary { '^' } else { '-' };
    let mut underline = String::new();
    underline.push_str(&" ".repeat(caret_start));
    for _ in 0..caret_len {
        underline.push(marker);
    }
    if !label.message.is_empty() {
        underline.push(' ');
        underline.push_str(&label.message);
    }
    // Trailing-whitespace-free underline row.
    let _ = writeln!(out, "{pad} | {}", underline.trim_end());
}

/// Renders to stderr; used by the driver.
pub fn eprint(source_map: &SourceMap, diagnostics: &Diagnostics) {
    let text = render_all(source_map, diagnostics);
    if !text.is_empty() {
        eprint!("{text}");
    }
}

/// Renders to stderr with a trailing summary line ("aborting due to N errors").
pub fn eprint_with_summary(source_map: &SourceMap, diagnostics: &Diagnostics) {
    eprint(source_map, diagnostics);
    let n = diagnostics.error_count();
    if n > 0 {
        let plural = if n == 1 { "" } else { "s" };
        eprintln!("aborting due to {n} previous error{plural}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_span::{BytePos, SourceMap};

    fn fixture() -> (SourceMap, BytePos) {
        let mut sm = SourceMap::new();
        let fid = sm.add_file("t.rill", "let x = foo + 1;\nlet y = bar;\n".to_string());
        let start = sm.file(fid).start;
        (sm, start)
    }

    #[test]
    fn renders_location_line_and_caret() {
        let (sm, start) = fixture();
        // `foo` is at line 1, col 9.
        let span = Span::new(BytePos(start.0 + 8), BytePos(start.0 + 11));
        let diag = Diagnostic::error(span, "unknown identifier `foo`")
            .primary_label(span, "not found")
            .note("did you mean `bar`?");
        let out = render(&sm, &[diag]);
        assert!(out.contains("error: unknown identifier `foo`"), "{out}");
        assert!(out.contains("--> t.rill:1:9"), "{out}");
        assert!(out.contains("1 | let x = foo + 1;"), "{out}");
        assert!(out.contains("^^^ not found"), "{out}");
        assert!(out.contains("= note: did you mean `bar`?"), "{out}");
    }

    #[test]
    fn multi_line_span_renders_both_lines() {
        let (sm, start) = fixture();
        let span = Span::new(BytePos(start.0 + 8), BytePos(start.0 + 26));
        let diag = Diagnostic::error(span, "spans two lines");
        let out = render(&sm, &[diag]);
        assert!(out.contains("1 | let x = foo + 1;"), "{out}");
        assert!(out.contains("2 | let y = bar;"), "{out}");
        assert!(out.contains("..."), "{out}");
    }

    #[test]
    fn accumulates_counts() {
        let (_, start) = fixture();
        let mut diags = Diagnostics::new();
        diags.error(Span::new(start, start), "boom");
        diags.warning(Span::new(start, start), "hmm");
        assert!(diags.has_errors());
        assert_eq!(diags.error_count(), 1);
        assert_eq!(diags.warning_count(), 1);
        assert_eq!(diags.len(), 2);
    }
}
