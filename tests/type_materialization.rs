use rill::{check, parser};

const RESOURCE_ERROR: &str =
    "E-TYPE-COMPLEXITY: type materialization budget exceeded; simplify the program";

fn duplication(layers: usize, body: &str) -> String {
    let mut source = "type Pair<a,b> = Pair(a,b)\nfn f0(x){Pair(x,x)}\n".to_owned();
    for layer in 1..=layers {
        source += &format!("fn f{layer}(x){{f{}(f{}(x))}}\n", layer - 1, layer - 1);
    }
    source + &format!("fn main(){{{body}}}")
}

#[test]
fn nearby_tractable_schemes_and_expression_identity_are_preserved() {
    let source = duplication(4, "42");
    assert_eq!(source.len(), 140);
    let program = parser::parse(&source).unwrap();
    let checked = check::analyze(&program).unwrap();
    assert!(std::ptr::eq(checked.program, &program));
    for layer in 0..=4 {
        let name = format!("f{layer}");
        let signature = &checked.functions[&name];
        assert_eq!(signature.quantified.len(), 1);
        assert_eq!(
            signature.params,
            vec![check::Type::Var(signature.quantified[0])]
        );
        let mut pending = vec![(&signature.result, 1usize << layer)];
        while let Some((ty, depth)) = pending.pop() {
            if depth == 0 {
                assert_eq!(ty, &signature.params[0]);
            } else {
                let check::Type::Nominal(name, args) = ty else {
                    panic!("Pair expected")
                };
                assert_eq!(name, "Pair");
                assert_eq!(args.len(), 2);
                pending.extend(args.iter().map(|arg| (arg, depth - 1)));
            }
        }
        let function = program
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert_eq!(
            checked.expression_type(&function.body),
            Some(&signature.result)
        );
    }
}

#[test]
fn tractable_generic_execution_still_agrees_with_typed_reference_and_c() {
    let mut pattern = "x".to_owned();
    for _ in 0..8 {
        pattern = format!("Pair({pattern},_)");
    }
    for source in [
        duplication(3, "42"),
        duplication(3, &format!("match f3(42) {{ {pattern} => x }}")),
    ] {
        compare_execution(&source);
    }
}

fn compare_execution(source: &str) {
    let program = parser::parse(source).unwrap();
    let checked = check::analyze(&program).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap();
    let reference = rill::interpreter::run_reference(&program).unwrap();
    assert_eq!(reference.to_string(), "42");
    assert_eq!(rill::typed::evaluate(&typed).unwrap(), reference);
    assert_eq!(rill::interpreter::run(&program).unwrap(), reference);
    let c = rill::typed::emit_c(&typed).unwrap();
    assert_eq!(rill::backend::emit(&program).unwrap(), c);
    let dir = std::env::temp_dir().join(format!(
        "rill-type-materialization-c-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.c");
    let executable = dir.join("main");
    std::fs::write(&path, c).unwrap();
    for optimization in ["-O0", "-O2"] {
        let output = std::process::Command::new("cc")
            .args(["-std=c11", "-pedantic-errors", optimization])
            .arg(&path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = std::process::Command::new(&executable).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"42\n");
        assert!(output.stderr.is_empty());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_tractable_programs_keep_real_errors() {
    for (body, error) in [
        ("true + 1", "E-TYPE:"),
        ("missing", "E-NAME:"),
        ("f0(1,2)", "E-ARITY:"),
    ] {
        let program = parser::parse(&duplication(3, body)).unwrap();
        let actual = check::analyze(&program).err().unwrap();
        assert!(actual.starts_with(error), "{actual}");
        assert_ne!(actual, RESOURCE_ERROR);
    }
}

// These specific historical OOM and cumulative-copy probes always run in a
// subprocess with hard resource limits. No explosive in-process test fallback.
#[cfg(target_os = "linux")]
#[test]
fn explosive_and_repeated_sources_return_diagnostics_under_process_limits() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let dir = std::env::temp_dir().join(format!(
        "rill-type-materialization-budget-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let cases = [
        (duplication(4, "42"), true),
        (duplication(5, "42"), false),
        (
            duplication(4, "let a=f4(0);let b=f4(true);let c=f4(1);42"),
            false,
        ),
    ];
    assert_eq!(cases[1].0.len(), 160);
    for (index, (source, accepted)) in cases.iter().enumerate() {
        let path = dir.join(format!("case{index}.rill"));
        std::fs::write(&path, source).unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "set -e; ulimit -v 196608; ulimit -t 5; ulimit -s 8192; ulimit -c 0; exec \"$1\" check \"$2\"", "rill-type-budget"])
            .arg(env!("CARGO_BIN_EXE_rill")).arg(&path)
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let start = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(7) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("bounded materialization probe exceeded seven seconds");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.success(), *accepted, "case {index}: {stderr}");
        if !accepted {
            assert!(stderr.contains(RESOURCE_ERROR), "case {index}: {stderr}");
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
