use std::{fs, process::Command};

#[test]
fn inout_preserves_snapshots_and_nested_argument_order() {
    let cases = [
        "let update previous (inout n) = n <- previous\nlet main = let mut x = 1 in let unused = update((let set = x <- 4 in x), inout x) in x",
        "let mutate (inout n) = n <- n + 1\nlet combine (inout x) n = x <- x + n\nlet main = let mut x = 1 in let mut y = 2 in let unused = combine(inout x, (let set = mutate(inout y) in y)) in x * 10 + y",
        "let mutate (inout n) = n <- n + 1\nlet forward (inout n) = mutate(inout n)\nlet main = let mut x = 1 in let old = x in let unused = forward(inout x) in old * 10 + x",
    ];
    let dir = std::env::temp_dir().join(format!("rill-inout-audit-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("program.c");
    let executable = dir.join("program");
    for code in cases {
        let program = rill::parser::parse(code).unwrap();
        rill::check::check(&program).unwrap();
        let expected = rill::interpreter::run(&program).unwrap().to_string();
        fs::write(&source, rill::compile(code).unwrap()).unwrap();
        for opt in ["-O0", "-O2"] {
            let output = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(&executable).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, format!("{expected}\n").as_bytes());
        }
    }
    fs::remove_dir_all(dir).unwrap();
}
