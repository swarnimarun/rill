//! Retained C11 differential fixtures for closed recursive parameter constraints.
#[path = "support/native_launch.rs"]
mod native_launch;

use rill::{backend, check, interpreter, parser, typed};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
#[path = "support/recursive_signature_cases.rs"]
mod cases;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str, source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rill-recursive-signature-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("main.rill"), source).unwrap();
        Self(dir)
    }
    fn output(&self, command: &mut Command, label: &str, seconds: u64) -> Output {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let stdout = self.0.join(format!("{serial}-{label}.out"));
        let stderr = self.0.join(format!("{serial}-{label}.err"));
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap());
        let status = native_launch::status(command, &self.0, Duration::from_secs(seconds))
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        Output {
            status,
            stdout: fs::read(stdout).unwrap(),
            stderr: fs::read(stderr).unwrap(),
        }
    }
    fn guard(&self) {
        if let Some(guard) = std::env::var_os("K4_RESOURCE_GUARD") {
            let result = self.output(Command::new("bash").arg(guard), "guard", 5);
            if !result.status.success() {
                eprintln!(
                    "resource guard: {} {}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
                // Stop the whole serial harness, not merely the current case.
                // A failed floor check cannot launch a later C subprocess.
                std::process::exit(86);
            }
        }
    }
    fn run(&self, label: &str, c: &str, expected: &str) {
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        let source = self.0.join(format!("{label}.c"));
        fs::write(&source, c).unwrap();
        for opt in ["-O0", "-O2"] {
            self.guard();
            let executable = self.0.join(format!("{label}-{}", &opt[1..]));
            let build = self.output(
                Command::new(&compiler)
                    .args(["-std=c11", "-pedantic-errors", opt])
                    .arg(&source)
                    .arg("-o")
                    .arg(&executable),
                "compile",
                30,
            );
            assert!(
                build.status.success(),
                "{label}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            if !build.stderr.is_empty() {
                eprintln!("{label}: {}", String::from_utf8_lossy(&build.stderr));
            }
            self.guard();
            let run = self.output(&mut Command::new(&executable), "execute", 10);
            assert_eq!(run.status.code(), Some(0), "{label} {opt}");
            assert_eq!(
                run.stdout,
                format!("{expected}\n").as_bytes(),
                "{label} {opt}"
            );
            assert!(
                run.stderr.is_empty(),
                "{label} {opt}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            eprintln!(
                "SIGNATURE_C_PASS {} {label} {opt}",
                compiler.to_string_lossy()
            );
        }
    }
}
#[test]
fn all_closed_signature_cases_match_reference_typed_and_c11_execution() {
    for (label, source, expected) in cases::cases() {
        let fixture = Fixture::new(&label, &source);
        let program = parser::parse(&source).unwrap();
        let checked = check::analyze(&program).unwrap();
        let lowered = typed::lower_main(&checked).unwrap();
        typed::verify(&lowered).unwrap();
        for result in [
            typed::evaluate(&lowered),
            interpreter::run(&program),
            interpreter::run_reference(&program),
        ] {
            assert_eq!(result.unwrap().to_string(), expected, "{label}");
        }
        let direct = typed::emit_c(&lowered).unwrap();
        assert_eq!(direct, backend::emit(&program).unwrap());
        assert!(
            !direct.contains("rl_value")
                && !direct.contains("RL_CAP")
                && !direct.contains("restrict")
        );
        fixture.run(&format!("{label}-typed"), &direct, expected);
        fixture.run(
            &format!("{label}-reference"),
            &backend::emit_reference(&program).unwrap(),
            expected,
        );
    }
}
