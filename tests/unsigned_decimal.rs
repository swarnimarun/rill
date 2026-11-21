use rill::interpreter::Value;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn source(n: i32) -> String {
    let base = include_str!("../examples/json/unsigned_decimal.rill");
    let (functions, _) = base.rsplit_once("\nlet main =").unwrap();
    format!("{functions}\nlet main = encode_unsigned({n})\n")
}

#[test]
fn round_trips_and_handles_boundaries() {
    for n in [0, 1, 9, 10, 999, 1000, 2_147_483_647, -1, i32::MIN] {
        let p = rill::parser::parse(&source(n)).unwrap();
        rill::check::check(&p).unwrap();
        let Value::Bytes(bytes) = rill::interpreter::run(&p).unwrap() else {
            panic!("expected bytes")
        };
        assert_eq!(bytes.len(), 11);
        if n < 0 {
            assert_eq!(bytes[0], 255);
            assert!(bytes[1..].iter().all(|&b| b == 0));
        } else {
            let start = bytes[0] as usize;
            assert!((1..=10).contains(&start));
            assert!(bytes[1..start].iter().all(|&b| b == 0));
            let text = std::str::from_utf8(&bytes[start..]).unwrap();
            assert_eq!(text, n.to_string());
            assert_eq!(text.parse::<i32>().unwrap(), n);
        }
    }
}

#[test]
fn output_matches_c_at_boundaries() {
    for n in [0, i32::MAX, -1] {
        let program = source(n);
        let p = rill::parser::parse(&program).unwrap();
        rill::check::check(&p).unwrap();
        let expected = rill::interpreter::run(&p).unwrap().to_string();
        let dir = std::env::temp_dir().join(format!(
            "rill-decimal-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let c_file = dir.join("decimal.c");
        let executable = dir.join("decimal");
        fs::write(&c_file, rill::compile(&program).unwrap()).unwrap();
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
                "{}",
                String::from_utf8_lossy(&build.stderr)
            );
            let result = Command::new(&executable).output().unwrap();
            assert!(result.status.success());
            assert_eq!(result.stdout, format!("{expected}\n").as_bytes());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
