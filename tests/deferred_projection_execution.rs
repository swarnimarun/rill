//! Retained C11 differential fixtures for deferred nominal record projections.
#[path = "support/native_launch.rs"]
mod native_launch;

use rill::{backend, check, interpreter, parser, typed};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[path = "support/deferred_projection_cases.rs"]
mod cases;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str, source: &str) -> Self {
        // Evidence is deliberately retained across runs. Container process IDs
        // can repeat, so they are not a unique invocation identity by themselves.
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rill-deferred-projection-{label}-{timestamp}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("main.rill"), source).unwrap();
        eprintln!("PROJECTION_C_ARTIFACTS {}", dir.display());
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
    fn run(&self, label: &str, c: &str, expected: Option<&str>) {
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
            match expected {
                Some(expected) => {
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
                }
                None => {
                    assert!(
                        !run.status.success(),
                        "{label} {opt}: expected a runtime trap"
                    );
                    assert!(
                        run.stdout.is_empty(),
                        "{label} {opt}: trap printed a result"
                    );
                    assert_eq!(run.stderr, b"rill: runtime trap\n", "{label} {opt}");
                }
            }
            eprintln!(
                "PROJECTION_C_PASS {} {label} {opt}",
                compiler.to_string_lossy()
            );
        }
    }
}
#[test]
fn deferred_projection_cases_match_reference_typed_and_c11_execution() {
    for case in cases::cases() {
        let (label, source, expected, reference_c) =
            (case.label, case.source, case.expected, case.reference_c);
        let fixture = Fixture::new(&label, &source);
        let program = parser::parse(&source).unwrap();
        check::check(&program).unwrap_or_else(|e| panic!("{label}: {e}"));
        let checked = check::analyze(&program).unwrap_or_else(|e| panic!("{label}: {e}"));
        let lowered = typed::lower_main(&checked).unwrap();
        typed::verify(&lowered).unwrap();
        for result in [
            typed::evaluate(&lowered),
            interpreter::run(&program),
            interpreter::run_reference(&program),
        ] {
            match expected {
                Some(expected) => assert_eq!(result.unwrap().to_string(), expected, "{label}"),
                None => assert!(result.is_err(), "{label}: expected a runtime trap"),
            }
        }
        let direct = typed::emit_c(&lowered).unwrap();
        assert_eq!(direct, backend::emit(&program).unwrap());
        assert!(
            !direct.contains("rl_value")
                && !direct.contains("RL_CAP")
                && !direct.contains("restrict")
        );
        fixture.run(&format!("{label}-typed"), &direct, expected);
        let reference = backend::emit_reference(&program);
        if reference_c {
            fixture.run(
                &format!("{label}-reference"),
                &reference.unwrap_or_else(|e| panic!("{label}: {e}")),
                expected,
            );
        } else {
            assert_eq!(
                reference.unwrap_err(),
                "E-C-ADT: unresolved polymorphic value requires local specialization",
                "{label}"
            );
            eprintln!("PROJECTION_REFERENCE_C_BOUNDARY {label}");
        }
    }
}
