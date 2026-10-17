//! Source positions, spans, interned symbols and source-file management.
//!
//! Intentionally dependency-free (see PLAN.md §7.1). All positions are byte
//! offsets into a global position space owned by a [`SourceMap`]; each file
//! occupies a disjoint `[start, end]` interval of that space.

use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// BytePos
// ---------------------------------------------------------------------------

/// A byte offset into the global position space of a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BytePos(pub u32);

impl BytePos {
    /// Sentinel used for synthesized (non-source) spans.
    pub const DUMMY: BytePos = BytePos(u32::MAX);

    #[inline]
    pub const fn new(raw: u32) -> Self {
        BytePos(raw)
    }

    #[inline]
    pub const fn to_u32(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn to_usize(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Debug for BytePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for BytePos {
    #[inline]
    fn from(raw: usize) -> Self {
        BytePos(raw as u32)
    }
}

// ---------------------------------------------------------------------------
// Span
// ---------------------------------------------------------------------------

/// Half-open byte range `[start, end)` within one file.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: BytePos,
    pub end: BytePos,
}

impl Span {
    pub const DUMMY: Span = Span {
        start: BytePos::DUMMY,
        end: BytePos::DUMMY,
    };

    #[inline]
    pub const fn new(start: BytePos, end: BytePos) -> Self {
        Span { start, end }
    }

    /// Builds a span from raw `usize` offsets, `start..end`.
    #[inline]
    pub fn range(start: usize, end: usize) -> Self {
        Span {
            start: BytePos(start as u32),
            end: BytePos(end as u32),
        }
    }

    #[inline]
    pub fn is_dummy(self) -> bool {
        self.start.0 == u32::MAX
    }

    /// Smallest span covering both; dummy spans defer to the other.
    #[inline]
    pub fn merge(self, other: Span) -> Span {
        if self.is_dummy() {
            return other;
        }
        if other.is_dummy() {
            return self;
        }
        Span {
            start: if self.start <= other.start {
                self.start
            } else {
                other.start
            },
            end: if self.end >= other.end {
                self.end
            } else {
                other.end
            },
        }
    }

    #[inline]
    pub fn len(self) -> usize {
        self.end.to_usize().saturating_sub(self.start.to_usize())
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.is_dummy() || self.end <= self.start
    }

    #[inline]
    pub fn contains(self, pos: BytePos) -> bool {
        !self.is_dummy() && self.start <= pos && pos < self.end
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_dummy() {
            write!(f, "<dummy>")
        } else {
            write!(f, "{}..{}", self.start.0, self.end.0)
        }
    }
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

/// An interned string, resolved through [`Interner::get`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(u32);

impl Symbol {
    /// Symbol `0` is reserved for the empty string.
    pub const EMPTY: Symbol = Symbol(0);

    #[inline]
    pub const fn from_u32(raw: u32) -> Self {
        Symbol(raw)
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbol({})", self.0)
    }
}

/// Append-only string interner. `Symbol`s are `u32` indices into `strings`.
#[derive(Default)]
pub struct Interner {
    strings: Vec<Box<str>>,
    map: HashMap<Box<str>, u32>,
}

impl Interner {
    /// Creates an interner with [`Symbol::EMPTY`] pre-interned.
    pub fn new() -> Self {
        let mut it = Interner::default();
        let empty = it.intern("");
        debug_assert_eq!(empty, Symbol::EMPTY);
        it
    }

    pub fn intern(&mut self, text: &str) -> Symbol {
        if let Some(&id) = self.map.get(text) {
            return Symbol(id);
        }
        let id = self.strings.len() as u32;
        let boxed: Box<str> = text.into();
        self.strings.push(boxed.clone());
        self.map.insert(boxed, id);
        Symbol(id)
    }

    /// Resolves a symbol. Panics if `sym` was not produced by this interner.
    #[inline]
    pub fn get(&self, sym: Symbol) -> &str {
        &self.strings[sym.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Source files and maps
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct FileId(pub u32);

/// A single source file registered in a [`SourceMap`].
pub struct SourceFile {
    pub id: FileId,
    pub name: Symbol,
    pub text: String,
    /// Global offset of byte 0 of this file.
    pub start: BytePos,
}

impl SourceFile {
    /// One past the last byte of this file.
    #[inline]
    pub fn end(&self) -> BytePos {
        BytePos(self.start.0 + self.text.len() as u32)
    }

    #[inline]
    pub fn contains(&self, pos: BytePos) -> bool {
        !pos_is_dummy(pos) && self.start <= pos && pos <= self.end()
    }
}

#[inline]
fn pos_is_dummy(pos: BytePos) -> bool {
    pos.0 == u32::MAX
}

/// 1-based line / column plus the global offset of the line start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
    pub line_start: BytePos,
}

/// Owns all source files and the symbol interner for a compilation session.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
    interner: Interner,
    next_start: u32,
}

impl SourceMap {
    pub fn new() -> Self {
        SourceMap {
            files: Vec::new(),
            interner: Interner::new(),
            next_start: 0,
        }
    }

    pub fn intern(&mut self, text: &str) -> Symbol {
        self.interner.intern(text)
    }

    pub fn symbol(&self, sym: Symbol) -> &str {
        self.interner.get(sym)
    }

    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// Registers a new file, assigning it a fresh range of the global
    /// position space. A one-byte gap is kept between files so that the EOF
    /// position of one file is never contained in its successor.
    pub fn add_file(&mut self, name: &str, text: String) -> FileId {
        let id = FileId(self.files.len() as u32);
        let start = BytePos(self.next_start);
        self.next_start = start.0 + text.len() as u32 + 1;
        let name = self.intern(name);
        self.files.push(SourceFile {
            id,
            name,
            text,
            start,
        });
        id
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// The file containing `pos`, if any.
    pub fn file_for(&self, pos: BytePos) -> Option<&SourceFile> {
        if pos_is_dummy(pos) {
            return None;
        }
        self.files.iter().find(|f| f.contains(pos))
    }

    /// Source text covered by `span`; empty for dummy / out-of-file spans.
    pub fn snippet(&self, span: Span) -> &str {
        if span.is_dummy() {
            return "";
        }
        let Some(file) = self.file_for(span.start) else {
            return "";
        };
        let start = (span.start.0 - file.start.0).min(file.text.len() as u32) as usize;
        let end = (span.end.0 - file.start.0).min(file.text.len() as u32) as usize;
        // `get` guards against non-char-boundary indices without panicking.
        file.text.get(start..end).unwrap_or("")
    }

    /// Computes the 1-based line/column of `pos`.
    pub fn line_col(&self, pos: BytePos) -> LineCol {
        let Some(file) = self.file_for(pos) else {
            return LineCol {
                line: 0,
                col: 0,
                line_start: BytePos(0),
            };
        };
        let rel = (pos.0 - file.start.0).min(file.text.len() as u32) as usize;
        let text = &file.text[..rel];
        let mut line = 1u32;
        let mut line_start = 0usize;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line += 1;
                line_start = i + 1;
            }
        }
        let col = file.text[line_start..rel].chars().count() as u32 + 1;
        LineCol {
            line,
            col,
            line_start: BytePos(file.start.0 + line_start as u32),
        }
    }

    /// The full text of the source line containing `pos`, without its newline.
    pub fn line_text(&self, pos: BytePos) -> &str {
        let Some(file) = self.file_for(pos) else {
            return "";
        };
        let lc = self.line_col(pos);
        let start = (lc.line_start.0 - file.start.0) as usize;
        let rest = &file.text[start..];
        let end = rest.find('\n').unwrap_or(rest.len());
        &rest[..end]
    }

    /// Human-readable `name:line:col` for the start of `span`.
    pub fn location(&self, span: Span) -> String {
        let Some(file) = self.file_for(span.start) else {
            return "<unknown>".to_string();
        };
        let lc = self.line_col(span.start);
        format!("{}:{}:{}", self.symbol(file.name), lc.line, lc.col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interner_roundtrip_and_dedup() {
        let mut it = Interner::new();
        let a = it.intern("hello");
        let b = it.intern("world");
        let a2 = it.intern("hello");
        assert_eq!(a, a2);
        assert_ne!(a, b);
        assert_eq!(it.get(a), "hello");
        assert_eq!(it.get(b), "world");
        assert_eq!(it.get(Symbol::EMPTY), "");
        assert_eq!(it.len(), 3);
    }

    #[test]
    fn line_col_and_lines() {
        let mut sm = SourceMap::new();
        let text = "line one\nline two\nline three".to_string();
        let fid = sm.add_file("t.rill", text);
        let start = sm.file(fid).start;
        let lc = sm.line_col(BytePos(start.0 + 9)); // start of "line two"
        assert_eq!((lc.line, lc.col), (2, 1));
        let lc = sm.line_col(BytePos(start.0 + 14)); // 't' of "two"
        assert_eq!((lc.line, lc.col), (2, 6));
        assert_eq!(sm.line_text(BytePos(start.0 + 14)), "line two");
        assert_eq!(sm.line_col(BytePos(start.0)).line, 1);
    }

    #[test]
    fn files_do_not_share_eof_positions() {
        let mut sm = SourceMap::new();
        let a = sm.add_file("a.rill", "abc".to_string());
        let b = sm.add_file("b.rill", "def".to_string());
        let a_end = sm.file(a).end();
        assert_eq!(sm.file_for(a_end).unwrap().id, a);
        assert_eq!(sm.file_for(sm.file(b).start).unwrap().id, b);
        assert_eq!(sm.snippet(Span::new(sm.file(a).start, a_end)), "abc");
    }

    #[test]
    fn snippet_clamps_and_handles_dummy() {
        let mut sm = SourceMap::new();
        let fid = sm.add_file("x.rill", "hello world".to_string());
        let start = sm.file(fid).start;
        assert_eq!(sm.snippet(Span::new(start, BytePos(start.0 + 5))), "hello");
        assert_eq!(sm.snippet(Span::DUMMY), "");
        // End beyond the file is clamped.
        assert_eq!(
            sm.snippet(Span::new(start, BytePos(start.0 + 100))),
            "hello world"
        );
    }

    #[test]
    fn span_merge_ignores_dummy() {
        let a = Span::range(3, 5);
        assert_eq!(Span::DUMMY.merge(a), a);
        assert_eq!(a.merge(Span::DUMMY), a);
        assert_eq!(
            Span::range(1, 2).merge(Span::range(4, 9)),
            Span::range(1, 9)
        );
        assert!(Span::DUMMY.is_dummy() && Span::DUMMY.is_empty());
    }
}
