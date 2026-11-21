use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn check_example(source: &str, expected: Option<&str>) {
    let program = rill::parser::parse(source).expect("example parses");
    rill::check::check(&program).expect("example type checks");
    let interpreted = rill::interpreter::run(&program);
    match expected {
        Some(value) => assert_eq!(interpreted.unwrap().to_string(), value),
        None => assert_eq!(interpreted.unwrap_err(), "runtime trap"),
    }

    let c = rill::compile(source).expect("example emits C");
    let dir = std::env::temp_dir().join(format!(
        "rill-normative-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let c_file = dir.join("example.c");
    let executable = dir.join("example");
    fs::write(&c_file, c).unwrap();
    for optimization in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&c_file)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("cc is required");
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let result = Command::new(&executable).output().unwrap();
        match expected {
            Some(value) => {
                assert!(result.status.success());
                assert_eq!(result.stdout, format!("{value}\n").as_bytes());
            }
            None => {
                assert!(!result.status.success());
                assert_eq!(result.stderr, b"rill: runtime trap\n");
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sequential_contract_examples_match_c() {
    for source in [
        include_str!("../examples/contracts/arithmetic.rill"),
        include_str!("../examples/contracts/short_circuit.rill"),
        include_str!("../examples/contracts/boundaries.rill"),
    ] {
        check_example(source, Some("42"));
    }
    check_example(include_str!("../examples/contracts/overflow.rill"), None);
}
