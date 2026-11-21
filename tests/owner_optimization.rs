//! Tests production allocation code under optimization without replacing
//! malloc/free in the generated source before compilation.
use std::{fs, path::Path, process::Command};

fn lto_supported(dir: &Path) -> bool {
    let source = dir.join("lto_probe.c");
    let binary = dir.join("lto_probe");
    fs::write(&source, "int main(void) { return 0; }\n").unwrap();
    Command::new("cc")
        .args(["-std=c11", "-O3", "-flto"])
        .arg(source)
        .arg("-o")
        .arg(binary)
        .output()
        .unwrap()
        .status
        .success()
}
fn has_undefined_symbol(output: &str, expected: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .any(|name| name.trim_start_matches('_').split('@').next() == Some(expected))
}
#[test]
fn allocation_and_failure_checks_survive_optimization() {
    let source = "let main = let a = system_box_i32_abort(20) in let b = system_box_i32_abort(22) in let moved = move a in box_read(moved) + box_read(b)";
    let generated = rill::compile(source).unwrap();
    let dir = std::env::temp_dir().join(format!("rill-owner-opt-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let cfile = dir.join("generated.c");
    let harness = dir.join("harness.c");
    let executable = dir.join("program");
    fs::write(&cfile, &generated).unwrap();
    fs::write(&harness, r#"#define main rill_generated_main
#include "generated.c"
#undef main
static int count;
static void *allocations[4];
static void *traced_allocate(size_t size) {
    int n = ++count;
    if (getenv("FAIL_FIRST") && n == 1) { fputs("A1-fail\n", stderr); return NULL; }
    if (getenv("FAIL_SECOND") && n == 2) { fputs("A2-fail\n", stderr); return NULL; }
    void *p = malloc(size);
    if (!p) return NULL;
    allocations[n] = p;
    fprintf(stderr, "A%d\n", n);
    return p;
}
static void traced_release(void *p) {
    int n = 1;
    while (n <= count && allocations[n] != p) ++n;
    fprintf(stderr, "F%d\n", n);
    free(p);
}
int main(void) { rl_allocate = traced_allocate; rl_release = traced_release; return rill_generated_main(); }
"#).unwrap();
    let mut configurations: Vec<Vec<&str>> = vec![vec!["-O0"], vec!["-O2"], vec!["-O3"]];
    if lto_supported(&dir) {
        configurations.push(vec!["-O3", "-flto"]);
    }
    for flags in configurations {
        // Compile the generated file unchanged. Retained undefined references
        // establish that the production allocator and release are still linked.
        let build = Command::new("cc")
            .arg("-std=c11")
            .args(&flags)
            .arg(&cfile)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{flags:?}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let symbols = Command::new("nm")
            .arg("-u")
            .arg(&executable)
            .output()
            .expect("nm is required for this test");
        assert!(
            symbols.status.success(),
            "{}",
            String::from_utf8_lossy(&symbols.stderr)
        );
        let symbols = String::from_utf8_lossy(&symbols.stdout);
        assert!(
            has_undefined_symbol(&symbols, "malloc"),
            "{flags:?}: missing malloc reference: {symbols}"
        );
        assert!(
            has_undefined_symbol(&symbols, "free"),
            "{flags:?}: missing free reference: {symbols}"
        );
        // The same unmodified generated code is included into a test harness;
        // only the public C entrypoint name is changed. The harness swaps the
        // allocator through the actual volatile callback slots at runtime.
        let build = Command::new("cc")
            .arg("-std=c11")
            .args(&flags)
            .arg(&harness)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{flags:?}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let normal = Command::new(&executable).output().unwrap();
        assert!(normal.status.success(), "{flags:?}");
        assert_eq!(normal.stdout, b"42\n");
        assert_eq!(normal.stderr, b"A1\nA2\nF1\nF2\n");
        for (variable, expected) in [
            ("FAIL_FIRST", "A1-fail\n"),
            ("FAIL_SECOND", "A1\nA2-fail\n"),
        ] {
            let result = Command::new(&executable)
                .env(variable, "1")
                .output()
                .unwrap();
            assert!(!result.status.success(), "{flags:?} {variable}");
            assert!(result.stdout.is_empty());
            assert_eq!(
                String::from_utf8_lossy(&result.stderr),
                format!("{expected}rill: runtime trap\n")
            );
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unused_reached_allocations_trap_but_untaken_branches_do_not_allocate() {
    let dir = std::env::temp_dir().join(format!("rill-owner-control-opt-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let cfile = dir.join("generated.c");
    let harness = dir.join("harness.c");
    let executable = dir.join("program");
    fs::write(
        &harness,
        r#"#define main rill_generated_main
#include "generated.c"
#undef main
static void *fail_allocate(size_t size) { (void)size; fputc('A', stderr); return NULL; }
int main(void) { rl_allocate = fail_allocate; return rill_generated_main(); }
"#,
    )
    .unwrap();
    let mut configurations: Vec<Vec<&str>> = vec![vec!["-O0"], vec!["-O2"], vec!["-O3"]];
    if lto_supported(&dir) {
        configurations.push(vec!["-O3", "-flto"]);
    }
    for (source, expected_stdout, expected_stderr, success) in [
        (
            "let main = let unused = system_box_i32_abort(42) in 0",
            "",
            "Arill: runtime trap\n",
            false,
        ),
        (
            "let main = if false then (let x = system_box_i32_abort(1) in box_read(x)) else 7",
            "7\n",
            "",
            true,
        ),
    ] {
        fs::write(&cfile, rill::compile(source).unwrap()).unwrap();
        for flags in &configurations {
            let build = Command::new("cc")
                .arg("-std=c11")
                .args(flags)
                .arg(&harness)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{flags:?}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let result = Command::new(&executable).output().unwrap();
            assert_eq!(result.status.success(), success, "{flags:?}: {source}");
            assert_eq!(
                result.stdout,
                expected_stdout.as_bytes(),
                "{flags:?}: {source}"
            );
            assert_eq!(
                result.stderr,
                expected_stderr.as_bytes(),
                "{flags:?}: {source}"
            );
        }
    }
    fs::remove_dir_all(dir).unwrap();
}
