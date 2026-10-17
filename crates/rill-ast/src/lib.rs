//! Rill AST: typed nodes, CST lowering, and computation-builder desugaring.
//!
//! Pipeline position (PLAN.md §7.2):
//!
//! ```text
//! tokens -> Cst (rill-parser) -> Module (lower) -> desugared Module (desugar)
//! ```
//!
//! [`parse_and_lower`] runs the whole front end for one file.

pub mod ast;
pub mod desugar;
pub mod lower;

pub use ast::*;
pub use lower::{lower_module, Lowered};

use rill_diagnostics::Diagnostics;
use rill_parser::parse_source;
use rill_span::{FileId, Interner, SourceMap};
use rill_syntax::Cst;

/// Everything the front end produced for one file.
pub struct Frontend {
    pub cst: Cst,
    pub module: Module,
    pub diagnostics: Diagnostics,
    pub interner: Interner,
}

impl Frontend {
    /// True when no phase reported an error.
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.has_errors()
    }

    /// Resolves an identifier to its text.
    pub fn text(&self, ident: Ident) -> &str {
        self.interner.get(ident.sym)
    }
}

/// Lexes, parses, lowers and desugars `file`.
pub fn parse_and_lower(source_map: &SourceMap, file: FileId) -> Frontend {
    let parsed = parse_source(source_map, file);
    let mut diagnostics = parsed.diagnostics;
    let mut lowered = lower_module(source_map, &parsed.cst);
    diagnostics.extend(std::mem::take(&mut lowered.diagnostics));
    Frontend {
        cst: parsed.cst,
        module: lowered.module,
        diagnostics,
        interner: lowered.interner,
    }
}

/// Convenience for tests and tools: parses `text` standalone.
pub fn parse_and_lower_text(name: &str, text: &str) -> (Frontend, SourceMap) {
    let mut sm = SourceMap::new();
    let fid = sm.add_file(name, text.to_string());
    let frontend = parse_and_lower(&sm, fid);
    (frontend, sm)
}
