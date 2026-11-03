use rill::{backend, check, interpreter, parser};
use std::{fs, process::Command};

#[test]
fn generic_calls_with_distinct_nested_instances_match_c() {
    let source = "type Option<a> = None | Some(a)\n\
        let wrap x = Some(x)\n\
        let main = let a = wrap(3) in wrap(a)";
    let program = parser::parse(source).unwrap();
    assert_eq!(check::analyze(&program).unwrap().max_inline_nodes(), Ok(3));
    let expected = interpreter::run(&program).unwrap().to_string();
    let c = backend::emit(&program).unwrap();
    let path = std::env::temp_dir().join(format!("rill-specialization-{}", std::process::id()));
    fs::create_dir(&path).unwrap();
    let src = path.join("program.c");
    let exe = path.join("program");
    fs::write(&src, c).unwrap();
    for opt in ["-O0", "-O2"] {
        let result = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let result = Command::new(&exe).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!("{expected}\n")
        );
    }
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn polymorphic_local_gets_a_concrete_type_at_its_use() {
    let source =
        "type Option<a> = None | Some(a)\nlet main = let n = None in if true then Some(1) else n";
    let c = rill::compile(source).unwrap();
    assert!(!c.contains("rl_value"));
    assert_eq!(
        interpreter::run(&parser::parse(source).unwrap())
            .unwrap()
            .to_string(),
        "Some(1)"
    );
}
