use std::{fs, process::Command};

#[test]
fn shared_root_loans_preserve_values_and_order() {
    let cases = [
        "let pair (read a) b = a + b\nlet main = let mut x = 10 in pair(x, (let mut x = 1 in let z = x <- 2 in x))",
        "let pair (inout a) b = a <- b\nlet main = let mut x = 1 in let z = pair(inout x, (let mut x = 2 in let ignored = x <- 3 in x)) in x",
        "type One = One(i32)\nlet pair (inout a) b = a <- b\nlet main = let mut x = 1 in let z = pair(inout x, match One(4) { One(x) => x }) in x",
        "let get (read x) = x\nlet add a b = a + b\nlet main = let mut x = 1 in add(get(x), (let z = x <- 2 in x))",
        "let sum (read a) (read b) = a + b\nlet main = let mut x = 4 in sum(x, x)",
        "let copied (read x) = let mut x = x in let ignored = x <- x + 1 in x\nlet main = let mut x = 5 in let y = copied(x) in x * 10 + y",
        "let add previous (read x) = previous + x\nlet main = let mut x = 1 in add((let update = x <- 7 in x), x)",
        "let observe (read x) = x\nlet forward (read x) = observe(x)\nlet modify (inout x) = x <- forward(x) + 1\nlet main = let mut x = 1 in let ignored = modify(inout x) in x",
        "type Box<a> = Box(a)\nlet identity (read x) = x\nlet main = let x = Box(bytes_set(bytes_new(256), 255, 99)) in match identity(x) { Box(b) => bytes_get(b, 255) }",
    ];
    let dir = std::env::temp_dir().join(format!("rill-read-audit-{}", std::process::id()));
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
