//! `rillc` — the Rill front-end driver (PLAN.md §7.1, M0 slice).
//!
//! Parses a source file into a lossless CST and a desugared AST, reporting
//! diagnostics. Code generation (M3) plugs into the same pipeline later.

use std::io::Write as _;
use std::process::ExitCode;

use rill_ast::parse_and_lower;
use rill_codegen::emit_c;
use rill_diagnostics::{eprint_with_summary, render_all};
use rill_hir::lower;
use rill_span::SourceMap;
use rill_ty::check;

const USAGE: &str = "\
rillc — Rill compiler

USAGE:
    rillc build <FILE.rill> [-o OUT] [--target native|linux-musl] [--emit-c]
    rillc <FILE.rill> [OPTIONS]

BUILD OPTIONS:
    -o OUT             output binary (default: the file stem in the cwd)
    --target native    host cc, dynamic — runs locally (default)
    --target linux-musl
                       zig cc -target <arch>-linux-musl -static — static ELF
    --emit-c           also print the generated C file's path
    --link LIB         link an extra library (`-lLIB`)

FRONT-END OPTIONS:
    --dump=<cst|ast|hir>
                       print the lossless CST, the AST or the resolved HIR
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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a.as_str()) == Some("build") {
        args.remove(0);
        return build(&args);
    }
    // `rillc fmt FILE` prints the canonical form of a file. `--write` edits it in
    // place, which is what an editor integration wants.
    if args.first().map(|a| a.as_str()) == Some("fmt") {
        args.remove(0);
        let write = args.iter().any(|a| a == "--write");
        let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
            eprintln!("rillc fmt FILE [--write]");
            return ExitCode::FAILURE;
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("rillc: {path}: {err}");
                return ExitCode::FAILURE;
            }
        };
        let formatted = rill_fmt::format_text(&text);
        if write {
            if let Err(err) = std::fs::write(path, &formatted) {
                eprintln!("rillc: {path}: {err}");
                return ExitCode::FAILURE;
            }
        } else {
            print!("{formatted}");
        }
        return ExitCode::SUCCESS;
    }
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
                    "cst" | "ast" | "hir" => dump = Some(value.to_string()),
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
        Some("hir") => {
            let hir = lower(&frontend);
            print_line(&format!("{hir:#?}"));
        }
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

/// `rillc build` — type-check, emit C, compile, link.
fn build(args: &[String]) -> ExitCode {
    let mut path = None;
    let mut out = None;
    let mut target = "native".to_string();
    let mut emit_c_only = false;
    let mut link_libs: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-o" => {
                index += 1;
                out = args.get(index).cloned();
            }
            "--target" => {
                index += 1;
                match args.get(index).map(|s| s.as_str()) {
                    Some(value @ ("native" | "linux-musl")) => target = value.to_string(),
                    other => {
                        eprintln!("error: unknown target {:?} (native or linux-musl)", other);
                        return ExitCode::from(2);
                    }
                }
            }
            "--emit-c" => emit_c_only = true,
            "--link" => {
                index += 1;
                match args.get(index) {
                    Some(lib) => link_libs.push(lib.clone()),
                    None => {
                        eprintln!("error: --link needs a library name");
                        return ExitCode::from(2);
                    }
                }
            }
            other => path = Some(other.to_string()),
        }
        index += 1;
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
    let mut diagnostics = frontend.diagnostics.clone();
    if diagnostics.has_errors() {
        eprint_with_summary(&source_map, &diagnostics);
        return ExitCode::FAILURE;
    }
    let hir = lower(&frontend);
    diagnostics.extend(hir.diagnostics.clone());
    if diagnostics.has_errors() {
        eprint_with_summary(&source_map, &diagnostics);
        return ExitCode::FAILURE;
    }
    let checked = check(&frontend, &hir);
    diagnostics.extend(checked.diagnostics.clone());
    if diagnostics.has_errors() {
        eprint_with_summary(&source_map, &diagnostics);
        return ExitCode::FAILURE;
    }

    let (c_source, codegen_diagnostics) = emit_c(&frontend, &hir, &checked);
    if codegen_diagnostics.has_errors() {
        eprint_with_summary(&source_map, &codegen_diagnostics);
        return ExitCode::FAILURE;
    }

    let stem = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "program".to_string());
    let out = out.unwrap_or_else(|| format!("./{stem}"));
    let c_path = format!("{out}.c");
    if let Err(err) = std::fs::write(&c_path, &c_source) {
        eprintln!("error: cannot write `{c_path}`: {err}");
        return ExitCode::from(2);
    }

    if emit_c_only {
        print_line(&c_path);
    }

    let links: String = link_libs.iter().map(|l| format!(" -l{l}")).collect();
    let cc = match target.as_str() {
        "linux-musl" => format!(
            "zig cc -target {}-linux-musl -static -O2 -o {} {}{}",
            std::env::consts::ARCH,
            out,
            c_path,
            links
        ),
        _ => format!("cc -O2 -o {out} {c_path}{links}"),
    };
    let status = std::process::Command::new("sh").arg("-c").arg(&cc).status();
    match status {
        Ok(status) if status.success() => {
            print_line(&format!("built {out} ({target})"));
            ExitCode::SUCCESS
        }
        Ok(status) => {
            eprintln!("error: `{cc}` failed with {status}");
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("error: cannot run `{cc}`: {err}");
            ExitCode::from(2)
        }
    }
}
