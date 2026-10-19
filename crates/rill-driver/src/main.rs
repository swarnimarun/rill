//! `rillc` — the Rill front-end driver (PLAN.md §7.1, M0 slice).
//!
//! Parses a source file into a lossless CST and a desugared AST, reporting
//! diagnostics. Code generation (M3) plugs into the same pipeline later.

use std::io::Write as _;
use std::process::ExitCode;

use rill_ast::parse_and_lower;
use rill_codegen::emit_c;
use rill_diagnostics::{eprint_with_summary, render_all};
use rill_hir::{lower_with, Options};
use rill_span::SourceMap;
use rill_ty::check;

const USAGE: &str = "\
rillc — Rill compiler

USAGE:
    rillc build <FILE.rill> [-o OUT] [--target native|linux-musl] [--emit-c]
    rillc exec <FILE.rill> [ARGS...]
                       compile into ~/.rill/cache and run it (rebuilds on change)
    rillc fmt <FILE|DIR>... [--write]
                       format Rill sources; a directory is searched for *.rill
                       (settings come from the nearest .rill-fmt)
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
    // `rill exec FILE [ARGS…]` compiles and runs a script, reusing a binary
    // cached under `~/.rill/cache` while the source and the compiler are
    // unchanged. Arguments after the file belong to the script.
    if args.first().map(|a| a.as_str()) == Some("exec") {
        args.remove(0);
        let Some(path) = args.first().cloned() else {
            eprintln!("rill exec FILE [ARGS...]");
            return ExitCode::from(2);
        };
        return exec(&path, &args[1..]);
    }
    // `rillc fmt FILE` prints the canonical form of a file. `--write` edits it in
    // place, which is what an editor integration wants. A directory formats every
    // `.rill` file below it, and each file loads the nearest `.rill-fmt`.
    // `rill lsp` is the interface an editor config is most likely to declare, so
    // the compiler offers it directly rather than requiring a second binary name.
    if args.first().map(|a| a.as_str()) == Some("lsp") {
        return match rill_lsp::run_stdio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    if args.first().map(|a| a.as_str()) == Some("fmt") {
        args.remove(0);
        let write = args.iter().any(|a| a == "--write");
        let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
        if paths.is_empty() {
            eprintln!("rillc fmt FILE... [--write]");
            return ExitCode::FAILURE;
        }
        // A directory formats every `.rill` file below it; an explicit file is
        // formatted on its own. The `.rill-fmt` nearest each file applies.
        let mut files = Vec::new();
        for path in &paths {
            collect_rill_files(std::path::Path::new(path), &mut files);
        }
        for file in files {
            let text = match std::fs::read_to_string(&file) {
                Ok(text) => text,
                Err(err) => {
                    eprintln!("rillc: {}: {err}", file.display());
                    return ExitCode::FAILURE;
                }
            };
            let config = match rill_fmt::Config::load_for(&file) {
                Ok(config) => config,
                Err(err) => {
                    eprintln!("rillc: {err}");
                    return ExitCode::FAILURE;
                }
            };
            let formatted = rill_fmt::format_text_with(&text, &config);
            if write {
                if let Err(err) = std::fs::write(&file, &formatted) {
                    eprintln!("rillc: {}: {err}", file.display());
                    return ExitCode::FAILURE;
                }
            } else {
                print!("{formatted}");
            }
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
    let mut frontend = parse_and_lower(&source_map, file);

    if frontend.diagnostics.has_errors() {
        eprint_with_summary(&source_map, &frontend.diagnostics);
    } else if !frontend.diagnostics.is_empty() {
        eprint!("{}", render_all(&source_map, &frontend.diagnostics));
    }

    match dump.as_deref() {
        Some("cst") => print_out(&frontend.cst.root.dump(&source_map)),
        Some("ast") => print_line(&format!("{:#?}", frontend.module)),
        Some("hir") => {
            let hir = lower_with(&mut frontend, &include_options(&path));
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
    let stem = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "program".to_string());
    let out = out.unwrap_or_else(|| format!("./{stem}"));
    let program = match analyse(&path) {
        Ok(program) => program,
        Err(code) => return code,
    };
    match emit_executable(&program, &out, &target, &link_libs, emit_c_only) {
        Ok(()) => {
            print_line(&format!("built {out} ({target})"));
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

/// A checked program: everything code generation and linking need.
struct Program {
    frontend: rill_ast::Frontend,
    hir: rill_hir::Hir,
    checked: rill_ty::Checked,
    source_map: SourceMap,
}

/// Parses, resolves and checks `path`, reporting diagnostics.
fn analyse(path: &str) -> Result<Program, ExitCode> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("error: cannot read `{path}`: {err}");
            return Err(ExitCode::from(2));
        }
    };

    let mut source_map = SourceMap::new();
    let file = source_map.add_file(path, text);
    let mut frontend = parse_and_lower(&source_map, file);
    let mut diagnostics = frontend.diagnostics.clone();
    if diagnostics.has_errors() {
        eprint_with_summary(&source_map, &diagnostics);
        return Err(ExitCode::FAILURE);
    }
    let hir = lower_with(&mut frontend, &include_options(path));
    diagnostics.extend(hir.diagnostics.clone());
    if diagnostics.has_errors() {
        eprint_with_summary(&source_map, &diagnostics);
        return Err(ExitCode::FAILURE);
    }
    let checked = check(&frontend, &hir);
    diagnostics.extend(checked.diagnostics.clone());
    if diagnostics.has_errors() {
        eprint_with_summary(&source_map, &diagnostics);
        return Err(ExitCode::FAILURE);
    }
    Ok(Program {
        frontend,
        hir,
        checked,
        source_map,
    })
}

fn emit_executable(
    program: &Program,
    out: &str,
    target: &str,
    link_libs: &[String],
    emit_c_path: bool,
) -> Result<(), ExitCode> {
    let (c_source, codegen_diagnostics) = emit_c(&program.frontend, &program.hir, &program.checked);
    if codegen_diagnostics.has_errors() {
        eprint_with_summary(&program.source_map, &codegen_diagnostics);
        return Err(ExitCode::FAILURE);
    }

    let c_path = format!("{out}.c");
    if let Err(err) = std::fs::write(&c_path, &c_source) {
        eprintln!("error: cannot write `{c_path}`: {err}");
        return Err(ExitCode::from(2));
    }
    if emit_c_path {
        print_line(&c_path);
    }

    // The libraries `@link("NAME")` names, and the headers `@include` read,
    // are resolved against the loader's own search paths, so a script links and
    // runs against a system-installed SDL3 without a build file.
    let includes = include_flags(&program.hir)?;
    let libraries = library_flags(&program.hir, link_libs)?;
    // The compiler writes a staging name and the binary is moved into place only
    // on success: `exec` reads the cache entry's existence as "no need to
    // compile", so a half-written binary from an interrupted link must never
    // appear under the final name.
    let staged = format!("{out}.tmp");
    let flags = format!("{includes}{libraries}");
    let cc = match target {
        "linux-musl" => format!(
            "zig cc -target {}-linux-musl -static -O2 -o {} {}{}",
            std::env::consts::ARCH,
            staged,
            c_path,
            flags
        ),
        _ => format!("cc -O2 -o {staged} {c_path}{flags}"),
    };
    match std::process::Command::new("sh").arg("-c").arg(&cc).status() {
        Ok(status) if status.success() => match std::fs::rename(&staged, out) {
            Ok(()) => Ok(()),
            Err(err) => {
                eprintln!("error: cannot move `{staged}` into place: {err}");
                Err(ExitCode::from(2))
            }
        },
        Ok(status) => {
            let _ = std::fs::remove_file(&staged);
            eprintln!("error: `{cc}` failed with {status}");
            Err(ExitCode::FAILURE)
        }
        Err(err) => {
            let _ = std::fs::remove_file(&staged);
            eprintln!("error: cannot run `{cc}`: {err}");
            Err(ExitCode::from(2))
        }
    }
}

/// Where `@include` looks for headers: the source file's own directory first,
/// then the compiler's defaults (`$CPATH`, the system prefixes).
fn include_options(path: &str) -> Options {
    let mut options = Options::default();
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            options.include_dirs.push(dir.to_path_buf());
        }
    }
    options
}

/// `-I` for every `@include`d header: the directory the written path is
/// relative to, so the generated C's `#include <…>`, spelled exactly as the
/// script wrote it, resolves to the same file.
fn include_flags(hir: &rill_hir::Hir) -> Result<String, ExitCode> {
    let mut flags = String::new();
    for include in hir.includes() {
        let Some(header) = include.headers.first() else {
            continue;
        };
        let written = std::path::Path::new(&include.path);
        if written.is_absolute() {
            continue;
        }
        let mut dir = header.clone();
        for _ in written.components() {
            if !dir.pop() {
                break;
            }
        }
        flags.push_str(&format!(" -I{}", dir.display()));
    }
    Ok(flags)
}

/// `-l` (and `-L`/`-rpath` when the library is not on the default search path)
/// for every `@link` value, plus the command line's own `--link` names.
fn library_flags(hir: &rill_hir::Hir, extra: &[String]) -> Result<String, ExitCode> {
    let mut flags = String::new();
    for library in hir.links() {
        let Some(dir) = find_library(library) else {
            eprintln!(
                "error: cannot find `{library}` in {}",
                library_dirs()
                    .iter()
                    .map(|dir| dir.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Err(ExitCode::FAILURE);
        };
        flags.push_str(&format!(
            " -L{} -l{library} -Wl,-rpath,{}",
            dir.display(),
            dir.display()
        ));
    }
    for library in extra {
        flags.push_str(&format!(" -l{library}"));
    }
    Ok(flags)
}

/// The directories a `@link` library is looked for in: the loader's own
/// environment, then the usual prefixes.
fn library_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    for var in ["LIBRARY_PATH", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(var) {
            for part in std::env::split_paths(&value) {
                if !part.as_os_str().is_empty() && !dirs.contains(&part) {
                    dirs.push(part);
                }
            }
        }
    }
    for prefix in ["/opt/homebrew/lib", "/usr/local/lib", "/usr/lib", "/lib"] {
        let dir = std::path::PathBuf::from(prefix);
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The directory holding `libNAME.dylib` (or `.so`).
fn find_library(name: &str) -> Option<std::path::PathBuf> {
    for dir in library_dirs() {
        for suffix in ["dylib", "so"] {
            if dir.join(format!("lib{name}.{suffix}")).is_file() {
                return Some(dir);
            }
        }
    }
    None
}

/// `rill exec FILE [ARGS…]` — compile the script if its checksum changed, then
/// run it.
///
/// The binary and the C source it was built from are cached under
/// `~/.rill/cache`; an unchanged script drops straight onto them and never
/// invokes the C toolchain.
fn exec(path: &str, program_args: &[String]) -> ExitCode {
    // The script is read and checked before the cache is consulted: the key
    // covers the headers its `@include`s read, and those are only known once
    // the front end has resolved them.
    let program = match analyse(path) {
        Ok(program) => program,
        Err(code) => return code,
    };
    let digest = fingerprint(path, &program.hir);
    let binary = match cache_path(path, &digest) {
        Ok(binary) => binary,
        Err(err) => {
            eprintln!("error: cannot prepare `~/.rill/cache`: {err}");
            return ExitCode::from(2);
        }
    };
    let out = binary.to_string_lossy().to_string();
    // Both artefacts live in the cache: the binary that runs and the C source
    // that produced it, which is what makes a stale or hand-deleted entry
    // rebuild instead of silently running half a cache.
    let cached_source = format!("{out}.c");
    if !binary.is_file() || !std::path::Path::new(&cached_source).is_file() {
        if let Err(code) = emit_executable(&program, &out, "native", &[], false) {
            return code;
        }
    }
    match std::process::Command::new(&binary)
        .args(program_args)
        .status()
    {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(err) => {
            eprintln!("error: cannot run `{out}`: {err}");
            ExitCode::from(2)
        }
    }
}

/// The checksum a cached build is keyed by: the script's text, the headers its
/// `@include`s resolved to, the libraries its `@link`s resolved to, and the
/// compiler that will build it. Editing any of them - or rebuilding `rill` -
/// rebuilds the script.
fn fingerprint(path: &str, hir: &rill_hir::Hir) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .hash(&mut hasher);
    fn stamp(hasher: &mut std::collections::hash_map::DefaultHasher, file: &std::path::Path) {
        file.to_string_lossy().hash(hasher);
        if let Ok(meta) = std::fs::metadata(file) {
            meta.len().hash(hasher);
            if let Ok(modified) = meta.modified() {
                if let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH) {
                    since.as_nanos().hash(hasher);
                }
            }
        }
    }
    for header in hir.imported_headers() {
        stamp(&mut hasher, header);
    }
    for library in hir.links() {
        // The *installed* library's identity is what a re-link needs: an
        // upgrade may change the install name the old binary recorded.
        match find_library(library) {
            Some(dir) => {
                for suffix in ["dylib", "so"] {
                    stamp(&mut hasher, &dir.join(format!("lib{library}.{suffix}")));
                }
            }
            None => library.hash(&mut hasher),
        }
    }
    format!("{:016x}", hasher.finish())
}

/// Where `exec` keeps a script's artefacts: `~/.rill/cache/<stem>-<digest>-<cc>`,
/// where `<digest>` is `fingerprint` - an unchanged script drops straight onto
/// its cached binary - and `<cc>` fingerprints the running compiler, so a
/// rebuilt `rill` does not run code the old one generated. `RILL_CACHE_DIR`
/// moves the directory, which is how a test keeps it hermetic.
fn cache_path(path: &str, digest: &str) -> std::io::Result<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let dir = match std::env::var_os("RILL_CACHE_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(".rill")
            .join("cache"),
    };
    std::fs::create_dir_all(&dir)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "rill-exec-v1".hash(&mut hasher);
    std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::metadata(exe).ok())
        .and_then(|meta| meta.modified().ok())
        .hash(&mut hasher);
    let compiler = hasher.finish() as u32;
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "script".to_string());
    Ok(dir.join(format!("{stem}-{digest}-{compiler:08x}")))
}

/// Every `.rill` file at `path`, or below it when it is a directory.
fn collect_rill_files(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<std::path::PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        children.sort();
        for child in children {
            collect_rill_files(&child, out);
        }
    } else if path.extension().and_then(|ext| ext.to_str()) == Some("rill") {
        out.push(path.to_path_buf());
    }
}
