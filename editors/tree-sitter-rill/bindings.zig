//! Zig access to the Rill tree-sitter grammar.
//!
//! The generated parser reaches consumers two ways: as the module built by
//! `build.zig` (which compiles `src/parser.c` into the importing artifact) and
//! as the query text below, embedded so a consumer cannot disagree with the
//! parser about which node names exist.

pub const highlights_scm = @embedFile("queries/highlights.scm");

/// The signature of the parser entry point. The returned pointer is a
/// `TSLanguage` from the tree-sitter runtime the consumer links, so it stays
/// `anyopaque` here: this package has no dependency on a tree-sitter binding
/// and the consumer casts to the type its own runtime uses.
pub const LanguageFn = *const fn () callconv(.c) ?*const anyopaque;

/// Declared by the generated `src/parser.c`; the entry point tree-sitter calls
/// to obtain the language.
pub extern fn tree_sitter_rill() ?*const anyopaque;
