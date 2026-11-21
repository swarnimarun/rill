use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn standard_result_with_owned_bytes_crosses_module_factories() {
    let dir = std::env::temp_dir().join(format!(
        "rill-owner-adt-module-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("factory.rill"), "import std.result as r; export let make n = if n > 0 then (let mut output = system_bytes_zeroed_abort(n) in let written = owned_bytes_set(inout output, 0, 42) in r.Ok(move output)) else r.Err(7)").unwrap();
    fs::write(
        dir.join("relay.rill"),
        "import factory; export let make n = factory.make(n)",
    )
    .unwrap();
    let source = "import std.result as r; import relay; let main = let result = relay.make(2) in match move result { r.Ok(bytes) => owned_bytes_get(bytes, 0) | r.Err(code) => code }";
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let p = rill::modules::compile::load_program(&entry).unwrap();
    rill::check::check(&p).unwrap();
    assert_eq!(rill::interpreter::run(&p).unwrap().to_string(), "42");
    let c = dir.join("program.c");
    let binary = dir.join("program");
    let object = dir.join("program.o");
    let fixture = dir.join("fixture.c");
    fs::write(&fixture, r#"#include <stdio.h>
#include <stdlib.h>
static void *allocation;
void *fixture_malloc(size_t n) { allocation = malloc(n); fputs("A1\n", stderr); return allocation; }
void fixture_free(void *p) { if (p != allocation) abort(); fputs("F1\n", stderr); allocation = NULL; free(p); }
int rl_generated_main(void);
int main(void) { return rl_generated_main(); }
"#).unwrap();
    fs::write(&c, rill::backend::emit(&p).unwrap()).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args([
                "-std=c11",
                opt,
                "-Dmain=rl_generated_main",
                "-Dmalloc=fixture_malloc",
                "-Dfree=fixture_free",
                "-c",
            ])
            .arg(&c)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let link = Command::new("cc")
            .arg(&object)
            .arg(&fixture)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let result = Command::new(&binary).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, b"42\n");
        assert_eq!(result.stderr, b"A1\nF1\n");
    }
    fs::remove_dir_all(dir).unwrap();
}
