use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn compare(source: &str, expected: &str) {
    let program = rill::parser::parse(source).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        expected
    );
    let code = rill::backend::emit(&program).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "rill-vector-audit-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let src = dir.join("main.c");
    let exe = dir.join("main");
    fs::write(&src, code).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let result = Command::new(&exe).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, format!("{expected}\n").as_bytes());
    }
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn vector_copies_survive_generic_calls_adt_and_borrows() {
    let source="type Holder = Holder(u8x16)\nlet identity x = x\nlet peek (read x) = u8x16_lane(x, 0)\nlet update (inout x) value = x <- value\nlet main = let mut b = bytes_set(bytes_new(16), 0, 17) in let mut v = u8x16_load(b, 0) in let old = identity(v) in let changed = b <- bytes_set(b, 0, 93) in let next = u8x16_load(b, 0) in let updated = update(inout v, next) in match Holder(old) { Holder(x) => peek(x) + peek(v) }";
    compare(source, "110");
}
#[test]
fn vector_arguments_snapshot_values_left_to_right() {
    let source="let main = let a = bytes_set(bytes_new(16), 0, 17) in let b = bytes_set(bytes_new(16), 0, 93) in let mut v = u8x16_load(a, 0) in let result = u8x16_xor(v, (let changed = v <- u8x16_load(b, 0) in v)) in u8x16_lane(result, 0)";
    compare(source, "76");
}
#[test]
fn vector_loans_and_nominal_types_are_checked() {
    for source in [
        "let use (read v) n = n\nlet main = let mut v = u8x16_load(bytes_new(16), 0) in use(v, (let changed = v <- u8x16_load(bytes_new(16), 0) in 0))",
        "let use (inout v) n = n\nlet main = let mut v = u8x16_load(bytes_new(16), 0) in use(inout v, u8x16_lane(v, 0))",
    ] {let program=rill::parser::parse(source).unwrap();assert!(rill::check::check(&program).unwrap_err().contains("E-LOAN"));}
    let program = rill::parser::parse("type Packet = Packet(u8x16)\nlet main = Packet(1)").unwrap();
    assert!(rill::check::check(&program).unwrap_err().contains("E-TYPE"));
}
