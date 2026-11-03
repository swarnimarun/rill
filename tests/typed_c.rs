use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn typed_primitive_c_matches_checked_interpreters() {
    for source in [
        "let main = 40 + 2",
        "let main = 2147483647 + 1",
        "let main = -2147483648 - 1",
        "let main = 2147483647 * 2",
        "let main = -2147483648 / -1",
        "let main = -2147483648 % -1",
        "let main = -7 / 3",
        "let main = -7 % 3",
        "let main = 7 / -3",
        "let main = 7 % -3",
        "let main = 1 / 0",
        "let main = 1 % 0",
        "let main = false && (1 / 0 == 0)",
        "let main = true || (1 / 0 == 0)",
        "let main = if false then 1 / 0 else 42",
        "let main = let mut x = 0 in let ignored = while x < 100 do x <- x + 1 in x",
        "let main = let mut x = 0 in (let a = x <- 1 in x) + (let b = x <- 2 in x)",
        "let main = let mut x = 0 in (let a = x <- 7 in x) / (let b = x <- 3 in x)",
        "let main = let mut x = 0 in let ignored = false && (let changed = x <- 1 in true) in x",
        "let main = () == ()",
        "let main = ()",
        "let main = bytes_new(0)",
        "let main = bytes_set(bytes_new(3), 1, 255)",
        "let main = bytes_get(bytes_set(bytes_new(256), 255, 200), 255)",
        "let main = bytes_len(bytes_new(256))",
        "let main = bytes_get(bytes_new(0), 0)",
        "let main = bytes_get(bytes_new(4), -1)",
        "let main = bytes_set(bytes_new(4), 4, 1)",
        "let main = bytes_set(bytes_new(4), 0, 256)",
        "let main = let mut b = bytes_new(2) in let saved = b in let done = b <- bytes_set(b, 1, 42) in bytes_get(saved, 1) + bytes_get(b, 1)",
        "let main = let mut b = bytes_new(1) in bytes_get(bytes_set(b, (let done = b <- bytes_set(b, 0, 1) in 0), 2), 0)",
        r#"let main = "hé\0x""#,
        r#"let main = byte_len("é\0x")"#,
        r#"let main = byte_at("é", 0)"#,
        r#"let main = byte_at("é", 1)"#,
        r#"let main = byte_at("", 0)"#,
        r#"let main = byte_at("a", -1)"#,
        r#"let main = byte_at("a", 1)"#,
        r#"let main = let mut s = "a" in let done = s <- "é" in byte_len(s)"#,
        "let count s = byte_len(s)\nlet main = count(\"hé\")",
        "let add x y = x + y\nlet main = add(19, 23)",
        "let check x = x == ()\nlet main = check(())",
        "let nop x = if x then () else ()\nlet main = nop(true)",
        "let add x y = x + y\nlet main = let mut x = 0 in add((let a = x <- 1 in x), (let b = x <- 2 in x))",
        "let recur n = if n == 0 then 0 else recur(n - 1)\nlet main = recur(30)",
        "let recur n = if n == 0 then 0 else recur(n - 1)\nlet main = recur(31)",
        "let even n = if n == 0 then true else odd(n - 1)\nlet odd n = if n == 0 then false else even(n - 1)\nlet main = even(10)",
    ] {
        let program = rill::parser::parse(source).unwrap();
        let checked = rill::check::analyze(&program).unwrap();
        let typed = rill::typed::lower_main(&checked).unwrap();
        let expected = rill::interpreter::run_reference(&program);
        assert_eq!(rill::interpreter::run(&program), expected, "{source}");
        assert_eq!(rill::typed::evaluate(&typed), expected, "{source}");
        let c = rill::typed::emit_c(&typed).unwrap();
        assert_eq!(rill::backend::emit(&program).unwrap(), c, "{source}");
        assert!(!c.contains("rl_value"));
        let dir = std::env::temp_dir().join(format!(
            "rill-typed-c-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let c_file = dir.join("main.c");
        let executable = dir.join("main");
        fs::write(&c_file, c).unwrap();
        for opt in ["-O0", "-O2"] {
            let build = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&c_file)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let result = Command::new(&executable).output().unwrap();
            match &expected {
                Ok(value) => {
                    assert!(result.status.success(), "{source}");
                    assert_eq!(result.stdout, format!("{value}\n").as_bytes(), "{source}");
                }
                Err(_) => {
                    assert!(!result.status.success(), "{source}");
                    assert_eq!(result.stderr, b"rill: runtime trap\n", "{source}");
                }
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn unsupported_typed_program_uses_existing_backend() {
    let program = rill::parser::parse("fn main() { let marker = u32(0); 42 }").unwrap();
    let checked = rill::check::analyze(&program).unwrap();
    assert!(rill::typed::lower_main(&checked)
        .unwrap_err()
        .contains("E-TYPED-UNSUPPORTED"));
    assert!(rill::backend::emit(&program).unwrap().contains("rl_value"));
}
