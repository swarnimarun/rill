use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn workspace() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rill-fallible-module-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn canonical_result_survives_aliases_forwarding_and_unrelated_types() {
    let dir = workspace();
    fs::write(dir.join("factory.rill"), "import std.alloc as memory; export let make n = let value = system_box_i32(n) in move value").unwrap();
    fs::write(
        dir.join("relay.rill"),
        "import factory as f; export let make n = let value = f.make(n) in move value",
    )
    .unwrap();
    fs::write(dir.join("unrelated.rill"), "export type Result<a, b> = Ok(a) | Err(b)\nexport type AllocError = InvalidLength | SizeOverflow | OutOfMemory\nexport let value = 0").unwrap();
    for imports in [
        "import unrelated; import relay;",
        "import relay; import unrelated;",
    ] {
        let source = format!("{imports} import std.result as outcome; import std.alloc as memory; type Result<a, b> = Ok(a) | Err(b)\ntype AllocError = InvalidLength | SizeOverflow | OutOfMemory\nlet main = let result = relay.make(42) in match move result {{ outcome.Ok(owner) => box_read(owner) | outcome.Err(error) => match error {{ memory.InvalidLength => 1 | memory.SizeOverflow => 2 | memory.OutOfMemory => 3 }} }}");
        let entry = dir.join("main.rill");
        fs::write(&entry, source).unwrap();
        let program = rill::modules::compile::load_program(&entry).unwrap();
        rill::check::check(&program).unwrap();
        assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "42");
        rill::backend::emit(&program).unwrap();
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_import_impostors_and_affine_violations_are_rejected() {
    let dir = workspace();
    let prefix = "import std.alloc as alloc; import std.result as r; ";
    let cases = [
        "let main = let result = system_box_i32(42) in 0".to_string(),
        "import std.result as r; let main = let result = system_bytes_zeroed(1) in 0".to_string(),
        format!("{prefix}type Result<a, b> = Ok(a) | Err(b)\nlet main = let value = alloc.box_i32(42) in match move value {{ Ok(x) => box_read(x) | Err(_) => 0 }}"),
        format!("{prefix}type AllocError = InvalidLength | SizeOverflow | OutOfMemory\nlet main = let value = alloc.box_i32(42) in match move value {{ r.Ok(x) => box_read(x) | r.Err(e) => match e {{ InvalidLength => 1 | SizeOverflow => 2 | OutOfMemory => 3 }} }}"),
        format!("{prefix}let main = let value = alloc.box_i32(42) in let copy = value in 0"),
        format!("{prefix}let main = let value = alloc.box_i32(42) in let other = move value in match move value {{ r.Ok(x) => box_read(x) | r.Err(_) => 0 }}"),
        format!("{prefix}let main = let value = alloc.box_i32(42) in let other = if true then move value else (let fresh = alloc.box_i32(1) in move fresh) in 0"),
        format!("{prefix}let escape (read value) = match move value {{ r.Ok(x) => (let n = box_read(x) in move x) | r.Err(_) => (let x = system_box_i32_abort(0) in move x) }}\nlet main = let value = alloc.box_i32(42) in let owner = escape(value) in box_read(owner)"),
    ];
    for source in cases {
        let entry = dir.join("main.rill");
        fs::write(&entry, &source).unwrap();
        if let Ok(program) = rill::modules::compile::load_program(&entry) {
            assert!(rill::check::check(&program).is_err(), "accepted {source}");
            assert!(
                rill::interpreter::run(&program).is_err(),
                "executed {source}"
            );
            assert!(rill::backend::emit(&program).is_err(), "emitted {source}");
        }
    }
    // Standalone parser users do not acquire canonical declarations by spelling them.
    let program = rill::parser::parse("type Result<a,b> = Ok(a) | Err(b)\ntype AllocError = InvalidLength | SizeOverflow | OutOfMemory\nlet main = let value = system_box_i32(42) in 0").unwrap();
    let error = rill::check::check(&program).unwrap_err();
    assert!(error.contains("std.alloc"), "{error}");
    fs::remove_dir_all(dir).unwrap();
}
