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
    let dir = std::env::temp_dir().join(format!(
        "rill-record-audit-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let src = dir.join("main.c");
    let exe = dir.join("main");
    fs::write(&src, rill::backend::emit(&program).unwrap()).unwrap();
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
fn record_writebacks_resolve_current_root_and_preserve_siblings() {
    compare("type Pair = { x: i32; y: i32 }\nlet main = let mut p = record Pair { x = 1; y = 2 } in let done = p.x <- (let reset = p <- record Pair { x = 7; y = 8 } in 9) in p.x * 10 + p.y", "98");
    compare("type Choice = Small(i32) | Big(i32, i32)\ntype Pair = { x: Choice; y: Choice }\nlet flip (inout a) (inout b) = let first = a <- Big(2, 3) in b <- Small(7)\nlet main = let mut p = record Pair { x = Small(1); y = Big(8, 9) } in let done = flip(inout p.x, inout p.y) in match p.x { Small(n) => n | Big(a, b) => match p.y { Small(c) => a + b + c | Big(c, d) => c + d } }", "12");
    compare("type Pair = { x: i32; y: i32 }\nlet update (inout x) n = x <- n\nlet main = let mut p = record Pair { x = 1; y = 2 } in let done = update(inout p.x, (let mut p = record Pair { x = 3; y = 4 } in let z = p.x <- 9 in p.x)) in p.x * 10 + p.y", "92");
}

#[test]
fn module_record_interfaces_cannot_expose_private_types() {
    let dir = std::env::temp_dir().join(format!(
        "rill-record-module-audit-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("main.rill"), "import a; let main = 0").unwrap();
    for library in [
        "type Secret = { value: i32 }\nexport let get (read p: Secret) = p.value",
        "type Secret = { value: i32 }\nexport type Public = { inner: Secret }",
    ] {
        fs::write(dir.join("a.rill"), library).unwrap();
        let error = rill::modules::compile::load_program(&dir.join("main.rill")).unwrap_err();
        assert!(
            error.to_string().contains("E-MODULE-PRIVATE-TYPE"),
            "{error}"
        );
    }
    fs::remove_dir_all(dir).unwrap();
}
