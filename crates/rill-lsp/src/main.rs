//! The Rill language server.
//!
//! It speaks JSON-RPC over stdio, holds the analysed state of every open document,
//! and answers with diagnostics. Analysis is the same pipeline the compiler uses -
//! parse, resolve, check - so the editor sees exactly what a build would.

mod json;
mod server;

use server::Server;

fn main() -> std::process::ExitCode {
    let mut server = Server::new();
    if server.run().is_err() {
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
