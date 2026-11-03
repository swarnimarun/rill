use std::{fs, process::Command};

#[test]
fn rejects_wide_inline_frame_before_c_compilation() {
    let mut source = String::from("type Pair<a> = Pair(a, a)\nlet main = ");
    // Eleven compact layers exceed the conservative typed-frame bound.
    // Ten layers fit now that scalar temporaries no longer use RL_CAP.
    for i in 0..11 {
        let previous = if i == 0 {
            "0".to_string()
        } else {
            format!("v{}", i - 1)
        };
        source.push_str(&format!("let v{i} = Pair({previous}, {previous}) in "));
    }
    source.push_str("0\n");
    let program = rill::parser::parse(&source).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "0");
    assert!(rill::compile(&source).unwrap_err().contains("E-C-STACK"));
}

#[test]
fn ordinary_program_runs_on_a_constrained_stack() {
    let c = rill::compile("let main = 42").unwrap();
    let dir = std::env::temp_dir().join(format!("rill-stack-budget-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("program.c");
    let executable = dir.join("program");
    fs::write(&source, c).unwrap();
    let build = Command::new("cc")
        .args(["-std=c11", "-O0"])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let output = Command::new("sh")
        .args(["-c", "ulimit -s 128; exec \"$1\"", "sh"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"42\n");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn typed_backend_rejects_excessive_estimated_frame() {
    fn tree(depth: usize) -> String {
        if depth == 0 {
            return "0".into();
        }
        let branch = tree(depth - 1);
        format!("if true then {branch} else {branch}")
    }
    let source = format!("let main = {}", tree(13));
    let program = rill::parser::parse(&source).unwrap();
    let checked = rill::check::analyze(&program).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap();
    assert!(rill::typed::emit_c(&typed)
        .unwrap_err()
        .contains("E-C-STACK"));
    assert!(rill::backend::emit(&program)
        .unwrap_err()
        .contains("E-C-STACK"));
}
