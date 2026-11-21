use std::{fs, process::Command};

fn checked(source: &str) -> Result<(), String> {
    rill::check::check(&rill::parser::parse(source).unwrap())
}

#[test]
fn nested_loans_restore_the_enclosing_loan_and_scopes() {
    let accepted = [
        // The nested read ends before the outer exclusive argument starts.
        "let get (read a) = a\nlet set value (inout a) = a <- value\nlet main = let mut x = 3 in set(get(x), inout x)",
        // A shared loan stays active while another shared loan comes and goes.
        "let get (read a) = a\nlet add (read a) b = a + b\nlet main = let mut x = 3 in add(x, get(x))",
        // A different local with the same name can be updated while x is borrowed.
        "let put (inout a) value = a <- value\nlet main = let mut x = 10 in let u = put(inout x, (let mut x = 0 in let loop = while x < 3 do x <- x + 1 in x)) in x",
        // Match pattern names receive distinct identities, too.
        "type One = One(i32)\nlet add (read a) b = a + b\nlet main = let mut x = 10 in add(x, match One(3) { One(x) => x })",
    ];
    let rejected = [
        // Releasing a nested shared loan must not release the outer one.
        "let get (read a) = a\nlet add (read a) b = a + b\nlet main = let mut x = 1 in add(x, (let q = get(x) in let z = x <- 2 in q))",
        // Branches and loop bodies are checked even when unreachable.
        "let add (read a) b = a + b\nlet main = let mut x = 1 in add(x, if false then (let z = x <- 2 in 0) else 0)",
        "let add (read a) b = a + b\nlet main = let mut x = 1 in add(x, (let z = while false do x <- 2 in 0))",
        // A shadow initializer resolves to the still-borrowed outer x.
        "let put (inout a) b = a <- b\nlet main = let mut x = 1 in put(inout x, (let x = x in x))",
        // A nested exclusive loan cannot be created while a shared loan is active.
        "let bump (inout a) = a <- a + 1\nlet add (read a) b = a + b\nlet main = let mut x = 1 in add(x, (let z = bump(inout x) in 0))",
    ];
    for source in accepted {
        assert!(checked(source).is_ok(), "{source}");
    }
    for source in rejected {
        assert!(checked(source).unwrap_err().contains("E-LOAN"), "{source}");
    }
    // A rejected analysis cannot leak an active loan into a later analysis.
    assert!(checked("let main = 1").is_ok());
}

#[test]
fn shadowed_loans_execute_like_c() {
    let source = "let put (inout a) value = a <- value\nlet main = let mut x = 10 in let u = put(inout x, (let mut x = 0 in let loop = while x < 3 do x <- x + 1 in x)) in x";
    let program = rill::parser::parse(source).unwrap();
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "3");
    let dir = std::env::temp_dir().join(format!("rill-loan-scopes-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let src = dir.join("program.c");
    let exe = dir.join("program");
    fs::write(&src, rill::compile(source).unwrap()).unwrap();
    for opt in ["-O0", "-O2"] {
        let compile = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = Command::new(&exe).output().unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"3\n");
    }
    fs::remove_dir_all(dir).unwrap();
}
