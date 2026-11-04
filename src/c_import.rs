//! Source-selected C headers and libraries. Tool arguments never pass through a shell.
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) mod abi;
#[cfg(unix)]
pub mod configuration;
pub mod inputs;
pub mod selected;

struct CheckDirectory(std::path::PathBuf);
impl CheckDirectory {
    fn create() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let root = std::env::temp_dir();
            let root = if root.is_absolute() {
                root
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("cannot resolve temporary directory: {e}"))?
                    .join(root)
            };
            let path = root.join(format!(
                "rill-c-check-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("E-C-IMPORT: cannot create check directory: {e}")),
            }
        }
    }
}
impl Drop for CheckDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub includes: Vec<String>,
    pub links: Vec<String>,
    pub packages: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolved {
    pub cflags: Vec<OsString>,
    pub link_args: Vec<OsString>,
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        for header in &self.includes {
            if header.is_empty()
                || !header
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_./-".contains(&c))
            {
                return Err(format!("E-C-IMPORT: invalid include name {header:?}"));
            }
        }
        for (kind, names) in [
            ("library", &self.links),
            ("pkg-config package", &self.packages),
        ] {
            for name in names {
                if name.is_empty()
                    || name.starts_with('-')
                    || !name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_+.-".contains(&c))
                {
                    return Err(format!("E-C-IMPORT: invalid {kind} name {name:?}"));
                }
            }
        }
        Ok(())
    }

    /// Resolve only the packages explicitly requested by the source.
    pub fn resolve(&self) -> Result<Resolved, String> {
        self.validate()?;
        let mut result = Resolved::default();
        if !self.packages.is_empty() {
            result.cflags = pkg_config("--cflags", &self.packages)?;
            result.link_args = pkg_config("--libs", &self.packages)?;
        }
        // Source links supplement pkg-config's dependency graph. Preserve its
        // ordered output, including repeated libraries needed by static cycles.
        // Explicit CLI linker arguments are appended separately by the caller.
        for name in &self.links {
            let argument = OsString::from(format!("-l{name}"));
            if !result.link_args.contains(&argument) {
                result.link_args.push(argument);
            }
        }
        Ok(result)
    }

    pub fn preamble(&self) -> Result<String, String> {
        self.validate()?;
        Ok(self
            .includes
            .iter()
            .map(|header| format!("#include <{header}>\n"))
            .collect())
    }
}

fn pkg_config(option: &str, packages: &[String]) -> Result<Vec<OsString>, String> {
    let mut child = Command::new("pkg-config")
        .args([option, "--"])
        .args(packages)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("E-C-IMPORT: cannot start pkg-config: {e}"))?;
    let stdout = child.stdout.take().expect("piped pkg-config output");
    let stderr = child.stderr.take().expect("piped pkg-config diagnostics");
    let (status, stdout, stderr) = std::thread::scope(|scope| {
        let out = scope.spawn(|| read_bounded(stdout));
        let err = scope.spawn(|| read_bounded(stderr));
        (
            child.wait(),
            out.join().expect("pkg-config reader panicked"),
            err.join().expect("pkg-config reader panicked"),
        )
    });
    let status = status.map_err(|e| format!("E-C-IMPORT: cannot wait for pkg-config: {e}"))?;
    let (stdout, out_truncated) =
        stdout.map_err(|e| format!("E-C-IMPORT: cannot read pkg-config output: {e}"))?;
    let (stderr, err_truncated) =
        stderr.map_err(|e| format!("E-C-IMPORT: cannot read pkg-config diagnostics: {e}"))?;
    if !status.success() {
        return Err(format!(
            "E-C-IMPORT: pkg-config {}: {}{}",
            packages.join(" "),
            String::from_utf8_lossy(&stderr).trim(),
            if err_truncated {
                "\n[diagnostics truncated]"
            } else {
                ""
            }
        ));
    }
    if out_truncated {
        return Err("E-C-IMPORT: pkg-config output exceeds 64 KiB".into());
    }
    let flags =
        std::str::from_utf8(&stdout).map_err(|_| "E-C-IMPORT: pkg-config output is not UTF-8")?;
    split_flags(flags).map(|flags| flags.into_iter().map(OsString::from).collect())
}

/// Retain at most 64 KiB, but keep draining to avoid blocking a verbose child.
fn read_bounded(mut input: impl Read) -> std::io::Result<(Vec<u8>, bool)> {
    const LIMIT: usize = 64 * 1024;
    let mut output = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 8192];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let kept = count.min(LIMIT - output.len());
        output.extend_from_slice(&buffer[..kept]);
        truncated |= kept != count;
    }
    Ok((output, truncated))
}

/// Decode pkg-config's shell-style quoting without evaluating substitutions,
/// redirects, variables, or commands. Every decoded word is a single argument.
fn split_flags(input: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for c in input.chars() {
        if escaped {
            word.push(c);
            escaped = false;
        } else if c == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            started = true;
        } else if c.is_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(c);
            started = true;
        }
    }
    if escaped || quote.is_some() {
        return Err("E-C-IMPORT: malformed quoted pkg-config output".into());
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// Preserve compile/target options without allowing them to replace the C11
/// compilation action. Linker arguments are passed separately by the CLI.
pub fn validate_compile_args(cflags: &[OsString]) -> Result<(), String> {
    let mut arguments = cflags.iter();
    while let Some(argument) = arguments.next() {
        let Some(text) = argument.to_str() else {
            if [
                "-I",
                "-D",
                "-U",
                "-isystem",
                "-iquote",
                "-idirafter",
                "-include",
                "-imacros",
                "-isysroot",
                "--sysroot=",
                "-B",
                "-F",
                "-iframework",
                "--gcc-toolchain=",
                "-resource-dir=",
                "-MF",
                "-MT",
                "-MQ",
                "-MJ",
            ]
            .iter()
            .any(|prefix| argument.as_encoded_bytes().starts_with(prefix.as_bytes()))
            {
                continue;
            }
            return Err(format!(
                "E-C-IMPORT: unsupported validation C argument {argument:?}"
            ));
        };
        if [
            "-I",
            "-D",
            "-U",
            "-isystem",
            "-iquote",
            "-idirafter",
            "-include",
            "-imacros",
            "-isysroot",
            "--sysroot",
            "-target",
            "--target",
            "-arch",
            "-B",
            "-F",
            "-iframework",
            "-iframeworkwithsysroot",
            "-iwithsysroot",
            "-isystem-after",
            "-iprefix",
            "-iwithprefix",
            "-iwithprefixbefore",
            "-imultilib",
            "--gcc-toolchain",
            "-resource-dir",
            "-MF",
            "-MT",
            "-MQ",
            "-MJ",
        ]
        .contains(&text)
        {
            let operand = arguments
                .next()
                .ok_or_else(|| format!("E-C-IMPORT: missing operand for {text}"))?;
            if operand.is_empty() {
                return Err(format!("E-C-IMPORT: empty operand for {text}"));
            }
        } else if !text.starts_with('-')
            || [
                "-",
                "--",
                "-E",
                "-M",
                "-MM",
                "-S",
                "-c",
                "-ansi",
                "-fsyntax-only",
                "-fdriver-only",
                "-###",
                "-cc1",
                "-cc1as",
                "-r",
                "-shared",
                "--shared",
                "-dynamiclib",
                "-bundle",
                "-emit-llvm",
                "-emit-ast",
                "--analyze",
                "-analyze",
                "-extract-api",
                "-rewrite-objc",
                "--preprocess",
                "--assemble",
                "--compile",
                "--dependencies",
                "--user-dependencies",
                "--syntax-only",
                "--target-help",
                "--precompile",
                "-precompile",
                "-version",
            ]
            .contains(&text)
            || text.starts_with("-o")
            || text.starts_with("-x")
            || text.starts_with("-X")
            || text.starts_with("-Wp,")
            || text.starts_with("--output")
            || text.starts_with("--language")
            || text.starts_with("--driver-mode")
            || text.starts_with("--config")
            || text.starts_with("--help")
            || text.starts_with("-help")
            || text.starts_with("--version")
            || text.starts_with("-ccc-")
            || text.starts_with("--autocomplete")
            || text.starts_with("-dump")
            || text.starts_with("-print-")
            || text.starts_with("--print-")
            || (text.starts_with("-std=") && text != "-std=c11")
            || (text.starts_with("--std=") && text != "--std=c11")
        {
            return Err(format!(
                "E-C-IMPORT: unsupported validation C argument {argument:?}; C11 compilation mode and output are controlled by Rill"
            ));
        }
    }
    Ok(())
}

/// Ask the selected target C compiler to check headers and ABI assertions.
/// The same target/sysroot flags must also be used for the final build.
pub fn validate_c(
    source: &str,
    compiler: &OsStr,
    zig_cc: bool,
    cflags: &[OsString],
) -> Result<(), String> {
    validate_c_in(source, compiler, zig_cc, cflags, None)
}

/// Check using an explicit child working directory without changing the caller.
pub fn validate_c_in(
    source: &str,
    compiler: &OsStr,
    zig_cc: bool,
    cflags: &[OsString],
    directory: Option<&std::path::Path>,
) -> Result<(), String> {
    validate_compile_args(cflags)?;
    let mut command = Command::new(compiler);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let check_directory;
    if zig_cc {
        command.arg("cc");
        // Some Zig versions do not support Clang's syntax-only driver path.
        // Compile an object to check the same declarations without linking.
        check_directory = Some(CheckDirectory::create()?);
        command
            .arg("-c")
            .arg("-o")
            .arg(check_directory.as_ref().unwrap().0.join("check.o"));
    } else {
        check_directory = None;
        command.arg("-fsyntax-only");
    }
    command.args(cflags);
    command.args(["-std=c11", "-x", "c"]);
    command.arg("-");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "E-C-IMPORT: cannot start C compiler {}: {e}",
                compiler.to_string_lossy()
            )
        })?;
    let mut stdin = child.stdin.take().expect("piped C compiler input");
    // Drain diagnostics while sending input, including when a bad header makes
    // the compiler reject a large translation unit before reading all of it.
    let stderr = child.stderr.take().expect("piped C compiler diagnostics");
    let (status, diagnostics, written) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(source.as_bytes()));
        let reader = scope.spawn(move || read_bounded(stderr));
        let status = child.wait();
        let written = writer.join().expect("C input writer panicked");
        (
            status,
            reader.join().expect("C diagnostic reader panicked"),
            written,
        )
    });
    let status = status.map_err(|e| format!("E-C-IMPORT: cannot wait for C compiler: {e}"))?;
    let (diagnostics, truncated) =
        diagnostics.map_err(|e| format!("E-C-IMPORT: cannot read C diagnostics: {e}"))?;
    drop(check_directory);
    if !status.success() {
        return Err(format!(
            "E-C-IMPORT: target C header/type checking failed:\n{}{}",
            String::from_utf8_lossy(&diagnostics),
            if truncated {
                "\n[diagnostics truncated]"
            } else {
                ""
            }
        ));
    }
    written.map_err(|e| format!("E-C-IMPORT: cannot send C declarations: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_are_bounded_but_fully_drained() {
        let data = vec![b'x'; 100_000];
        let mut input = std::io::Cursor::new(&data);
        let (retained, truncated) = read_bounded(&mut input).unwrap();
        assert_eq!(retained.len(), 64 * 1024);
        assert!(truncated);
        assert_eq!(input.position(), data.len() as u64);
    }

    #[test]
    fn package_flags_are_words_without_shell_execution() {
        assert_eq!(
            split_flags(r#"-I/a\ b '-L/c d' "-DNAME=a b" -lx $(touch /tmp/x)"#).unwrap(),
            [
                "-I/a b",
                "-L/c d",
                "-DNAME=a b",
                "-lx",
                "$(touch",
                "/tmp/x)"
            ]
        );
        assert!(split_flags("'unfinished").is_err());
        assert!(split_flags("trailing\\").is_err());
    }

    #[test]
    fn rejects_source_directive_injection() {
        for header in ["", "stdio.h>\n#error injection", "\"a.h\""] {
            assert!(Config {
                includes: vec![header.into()],
                ..Config::default()
            }
            .validate()
            .is_err());
        }
        assert!(Config {
            packages: vec!["--libs".into()],
            ..Config::default()
        }
        .validate()
        .is_err());
        assert_eq!(
            Config {
                includes: vec!["SDL3/SDL.h".into()],
                ..Config::default()
            }
            .preamble()
            .unwrap(),
            "#include <SDL3/SDL.h>\n"
        );
    }

    #[test]
    fn repeated_declarative_links_are_idempotent() {
        let resolved = Config {
            links: vec![
                "first".into(),
                "first".into(),
                "second".into(),
                "first".into(),
            ],
            ..Config::default()
        }
        .resolve()
        .unwrap();
        assert!(resolved.cflags.is_empty());
        assert_eq!(
            resolved.link_args,
            [OsString::from("-lfirst"), OsString::from("-lsecond")]
        );
    }

    #[test]
    fn compiler_compares_types_not_only_width() {
        let prefix = "#include <stdint.h>\n_Bool foreign(uint32_t x);\n";
        validate_c(&format!("{prefix}_Static_assert(__builtin_types_compatible_p(__typeof__(&foreign), _Bool (*)(uint32_t)), \"signature\");"), OsStr::new("cc"), false, &[]).unwrap();
        let error = validate_c(&format!("{prefix}_Static_assert(__builtin_types_compatible_p(__typeof__(&foreign), int (*)(int32_t)), \"signature\");"), OsStr::new("cc"), false, &[]).unwrap_err();
        assert!(error.contains("signature"));
    }
}
