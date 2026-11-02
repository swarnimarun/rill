use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: i32) {
    let program = rill::parser::parse(source).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        expected.to_string()
    );
    let dir = std::env::temp_dir().join(format!(
        "rill-read-temporary-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let c = dir.join("program.c");
    let binary = dir.join("program");
    fs::write(&c, rill::compile(source).unwrap()).unwrap();
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
        let run = Command::new(&binary).output().unwrap();
        assert!(run.status.success());
        assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn read_temporaries_survive_call_and_preserve_argument_order() {
    for (source, result) in [
        ("let f (read x) = x + 1\nlet main = f(41)",42),
        ("let f (read x) = x + 1\nlet main = let x = 20 in f(x + 21)",42),
        ("type Choice = None | Some(i32)\nlet f (read x) = match x { None => 42 | Some(n) => n }\nlet main = f(None)",42),
        ("let f (read x) (inout y) = y <- x + y\nlet main = let mut n = 1 in let done = f((let changed = n <- 2 in n), inout n) in n",4),
        ("let f (read x) (read y) = x * 10 + y\nlet main = let mut n = 1 in f((let changed = n <- 2 in n), n)",22),
    ] {compare(source,result);}
}

#[test]
fn temporary_expression_does_not_bypass_an_existing_exclusive_loan() {
    let source = "let f (inout x) (read y) = x <- y\nlet main = let mut x = 1 in f(inout x, x + 1)";
    let program = rill::parser::parse(source).unwrap();
    assert!(rill::check::check(&program).unwrap_err().contains("E-LOAN"));
    assert!(rill::compile(source).is_err());
}
