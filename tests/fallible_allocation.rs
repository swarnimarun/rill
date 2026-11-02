//! Exercise the production volatile allocator slots, including optimized builds.
use std::{fs, path::Path, process::Command};

const SOURCE: &str = include_str!("fixtures/fallible_mixed.rill");

fn load(dir: &Path, source: &str) -> rill::ast::Program {
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    rill::modules::compile::load_program(&entry).unwrap()
}

#[test]
fn failures_recover_and_cleanup_through_real_allocator_slots() {
    let dir = std::env::temp_dir().join(format!("rill-fallible-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let program = load(&dir, SOURCE);
    let checked = rill::check::analyze(&program).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap();
    rill::typed::verify(&typed).unwrap();
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "42");
    assert_eq!(rill::typed::evaluate(&typed).unwrap().to_string(), "42");
    fs::write(
        dir.join("generated.c"),
        rill::backend::emit(&program).unwrap(),
    )
    .unwrap();
    fs::write(dir.join("fixture.c"), r#"
#define main rill_generated_main
#include "generated.c"
#undef main
#include <string.h>
static void *live[16];
static int attempts;
static unsigned failures;
static void *allocate(size_t size) {
    int id = ++attempts;
    if (id > 7) abort();
    size_t expected = id == 4 ? sizeof(rl_owned_bytes) + 3 : id == 5 ? sizeof(rl_owned_bytes) : id == 7 ? sizeof(rl_owned_bytes) + 1 : sizeof(int32_t);
    if (size != expected) abort();
    fprintf(stderr, "A%d%s\n", id, failures & (1u << id) ? "-fail" : "");
    if (failures & (1u << id)) return NULL;
    live[id] = malloc(size);
    if (!live[id]) abort();
    memset(live[id], 0xa5, size);
    return live[id];
}
static void release(void *p) {
    if (!p) abort();
    int id = 1;
    while (id <= attempts && live[id] != p) ++id;
    if (id > attempts) abort();
    fprintf(stderr, "F%d\n", id);
    live[id] = NULL;
    free(p);
}
int main(void) {
    const char *mask = getenv("RILL_FAILURE_MASK");
    failures = mask ? (unsigned)strtoul(mask, NULL, 10) : 0;
    rl_allocate = allocate;
    rl_release = release;
    int status = rill_generated_main();
    for (int id = 1; id <= attempts; ++id) if (live[id]) abort();
    if (attempts != (failures & 2 ? 1 : 7)) abort();
    return status;
}
"#).unwrap();
    let probe = dir.join("probe.c");
    fs::write(&probe, "int main(void) { return 0; }\n").unwrap();
    let mut configurations = vec![vec!["-O0"], vec!["-O2"], vec!["-O3"]];
    if Command::new("cc")
        .args(["-O3", "-flto"])
        .arg(&probe)
        .arg("-o")
        .arg(dir.join("probe"))
        .output()
        .unwrap()
        .status
        .success()
    {
        configurations.push(vec!["-O3", "-flto"]);
    }
    for flags in configurations {
        let binary = dir.join("program");
        let build = Command::new("cc")
            .arg("-std=c11")
            .args(&flags)
            .arg(dir.join("fixture.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{flags:?}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        for mask in [0, 2, 4, 8, 16, 32, 64, 128, 4 | 8 | 16, 32 | 64 | 128] {
            let result = Command::new(&binary)
                .env("RILL_FAILURE_MASK", mask.to_string())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{flags:?}, mask {mask}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let expected = if mask & 2 != 0 {
                1000
            } else {
                42 + if mask & 4 != 0 { 10 } else { 0 } + if mask & 16 != 0 { 100 } else { 0 }
            };
            assert_eq!(
                result.stdout,
                format!("{expected}\n").as_bytes(),
                "{flags:?}, mask {mask}"
            );
            let mut trace = String::new();
            for id in 1..=if mask & 2 != 0 { 1 } else { 7 } {
                trace.push_str(&format!(
                    "A{id}{}\n",
                    if mask & (1 << id) != 0 { "-fail" } else { "" }
                ));
                if id == 3 {
                    for freed in [3, 2] {
                        if mask & (1 << freed) == 0 {
                            trace.push_str(&format!("F{freed}\n"));
                        }
                    }
                }
                if id == 4 && mask & 16 == 0 {
                    trace.push_str("F4\n");
                }
                if id == 7 && mask & 128 == 0 {
                    trace.push_str("F7\n");
                }
            }
            if mask & 2 == 0 {
                for id in [6, 5, 1] {
                    if mask & (1 << id) == 0 {
                        trace.push_str(&format!("F{id}\n"));
                    }
                }
            }
            assert_eq!(result.stderr, trace.as_bytes(), "{flags:?}, mask {mask}");
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn maximum_length_failure_does_not_allocate_gigabytes_and_retry_succeeds() {
    let dir = std::env::temp_dir().join(format!("rill-fallible-max-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = r#"import std.alloc as alloc; import std.result as r;
let main = let huge = alloc.bytes_zeroed(2147483647) in
 let score = match move huge { r.Ok(_) => 1000 | r.Err(error) => match error { alloc.OutOfMemory => 40 | alloc.InvalidLength => 2000 | alloc.SizeOverflow => 3000 } } in
 let retry = alloc.box_i32(2) in match move retry { r.Ok(owner) => score + box_read(owner) | r.Err(_) => 4000 }"#;
    let program = load(&dir, source);
    // This fixture always rejects the enormous request before calling malloc.
    // Interpreter failure injection and constrained sizing are unit-tested separately.
    fs::write(
        dir.join("generated.c"),
        rill::backend::emit(&program).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("fixture.c"),
        r#"
#define main rill_generated_main
#include "generated.c"
#undef main
static int attempts, releases;
static void *owner;
static void *allocate(size_t size) {
    ++attempts;
    if (attempts == 1) {
        if ((uintmax_t)size != (uintmax_t)sizeof(rl_owned_bytes) + INT32_MAX) abort();
        return NULL;
    }
    if (attempts != 2 || size != sizeof(int32_t)) abort();
    owner = malloc(size);
    if (!owner) abort();
    return owner;
}
static void release(void *p) {
    if (!p || p != owner || ++releases != 1) abort();
    free(p); owner = NULL;
}
int main(void) {
    rl_allocate = allocate; rl_release = release;
    int status = rill_generated_main();
    if (attempts != 2 || releases != 1 || owner) abort();
    return status;
}
"#,
    )
    .unwrap();
    let binary = dir.join("program");
    let build = Command::new("cc")
        .args(["-std=c11", "-O2"])
        .arg(dir.join("fixture.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let result = Command::new(&binary).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, b"42\n");
    assert!(result.stderr.is_empty());
    fs::remove_dir_all(dir).unwrap();
}
