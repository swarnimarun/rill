use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn exported_factories_forward_fresh_owners_from_borrowed_input() {
    let dir = std::env::temp_dir().join(format!(
        "rill-returned-owner-module-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("factory.rill"), "export let clone (read source) = let mut output = system_bytes_zeroed_abort(owned_bytes_len(source)) in let mut pos = 0 in let copy = while pos < owned_bytes_len(source) do let byte = owned_bytes_set(inout output, pos, owned_bytes_get(source, pos)) in pos <- pos + 1 in move output").unwrap();
    fs::write(
        dir.join("relay.rill"),
        "import factory; export let copy (read source) = factory.clone(source)",
    )
    .unwrap();
    let source = "import relay; let main = let mut input = system_bytes_zeroed_abort(2) in let write = owned_bytes_set(inout input, 1, 42) in let mut output = relay.copy(input) in let change = owned_bytes_set(inout output, 1, 7) in owned_bytes_len(output) == 2 && owned_bytes_get(output, 0) == 0 && owned_bytes_get(output, 1) == 7 && owned_bytes_get(input, 1) == 42";
    let entry = dir.join("main.rill");
    fs::write(&entry, source).unwrap();
    let p = rill::modules::compile::load_program(&entry).unwrap();
    rill::check::check(&p).unwrap();
    assert_eq!(rill::interpreter::run(&p).unwrap().to_string(), "true");
    let c = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&c, rill::backend::emit(&p).unwrap()).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&c)
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
        assert_eq!(result.stdout, b"true\n");
    }
    fs::remove_dir_all(dir).unwrap();
}
