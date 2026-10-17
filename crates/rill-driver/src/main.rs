//! `rillc` — the Rill front-end driver (PLAN.md §7.1, M0 slice).
//!
//! Parses a source file into a lossless CST and a desugared AST, reporting
//! diagnostics. Code generation (M3) plugs into the same pipeline later.

use std::io::Write as _;
use std::process::ExitCode;

use rill_ast::parse_and_lower;
use rill_diagnostics::{eprint_with_summary, render_all};
use rill_span::SourceMap;

const USAGE: &str = "\
rillc — Rill front end (M0: lexer, parser, CST, desugared AST)

USAGE:
    rillc [OPTIONS] <FILE.rill>

OPTIONS:
    --dump=<cst|ast>   print the lossless CST or the Debug form of the AST
    --stats            print item/statement/token counts
    -h, --help         show this message
";

/// Writes to stdout, ignoring `EPIPE` so `rillc | head` behaves.
fn print_out(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
}

fn print_line(text: &str) {
    print_out(text);
    print_out("\n");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_out(USAGE);
        return ExitCode::SUCCESS;
    }
    let mut dump = None;
    let mut stats = false;
    let mut path = None;
    for arg in &args {
        match arg.as_str() {
            "--stats" => stats = true,
            other if other.starts_with("--dump=") => {
                let value = &other["--dump=".len()..];
                match value {
                    "cst" | "ast" => dump = Some(value.to_string()),
                    _ => {
                        eprintln!("error: unknown dump format `{value}` (expected cst or ast)");
                        return ExitCode::from(2);
                    }
                }
            }
            other if other.starts_with('-') => {
                eprintln!("error: unknown option `{other}`\n\n{USAGE}");
                return ExitCode::from(2);
            }
            other => path = Some(other.to_string()),
        }
    }
    let Some(path) = path else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("error: cannot read `{path}`: {err}");
            return ExitCode::from(2);
        }
    };

    let mut source_map = SourceMap::new();
    let file = source_map.add_file(&path, text);
    let frontend = parse_and_lower(&source_map, file);

    if frontend.diagnostics.has_errors() {
        eprint_with_summary(&source_map, &frontend.diagnostics);
    } else if !frontend.diagnostics.is_empty() {
        eprint!("{}", render_all(&source_map, &frontend.diagnostics));
    }

    match dump.as_deref() {
        Some("cst") => print_out(&frontend.cst.root.dump(&source_map)),
        Some("ast") => print_line(&format!("{:#?}", frontend.module)),
        _ => {}
    }

    if stats {
        let items = frontend.module.items.len();
        let declarations = frontend
            .module
            .items
            .iter()
            .filter(|item| !matches!(item, rill_ast::Item::Stmt(_)))
            .count();
        let tokens = frontend.cst.tokens().len();
        print_line(&format!(
            "{}: {items} items ({declarations} declarations), {tokens} tokens, {} errors, {} warnings",
            path,
            frontend.diagnostics.error_count(),
            frontend.diagnostics.warning_count()
        ));
    }

    if frontend.diagnostics.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
