//! The Rill language server, as a library so the compiler can offer it as
//! `rill lsp` - the interface an editor config is most likely to declare.
//!
//! It speaks JSON-RPC over stdio: it holds the analysed state of every open
//! document and answers with diagnostics, hover, references, semantic tokens and
//! formatting, using the same pipeline a build runs.

pub mod json;
pub mod server;

pub use server::{Document, Server};

/// Runs the server on stdio until the client says `exit`.
pub fn run_stdio() -> std::io::Result<()> {
    let mut server = Server::new();
    server.run()
}
