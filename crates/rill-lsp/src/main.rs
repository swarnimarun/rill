//! The Rill language server.
//!
//! It speaks JSON-RPC over stdio, holds the analysed state of every open document,
//! and answers with diagnostics. Analysis is the same pipeline the compiler uses -
//! parse, resolve, check - so the editor sees exactly what a build would.

use rill_lsp::run_stdio;

fn main() -> std::process::ExitCode {
    if run_stdio().is_err() {
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
