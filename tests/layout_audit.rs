use std::{fs, process::Command};

#[test]
fn nested_generic_values_preserve_snapshots_in_c() {
    let cases = [
        "type Option<a> = None | Some(a)\nlet id x = x\nlet main = let mut b = bytes_new(256) in let old = Some(b) in let ignored = b <- bytes_set(b, 255, 123) in match old { Some(v) => bytes_get(v, 255) + bytes_get(id(b), 255) | None => 0 }",
        "type Option<a> = None | Some(a)\nlet id x = x\nlet main = let mut s = \"é\" in let old = Some(s) in let ignored = s <- \"a\" in match old { Some(v) => byte_at(id(v), 0) + byte_len(s) | None => 0 }",
    ];
    let dir = std::env::temp_dir().join(format!("rill-layout-audit-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("program.c");
    let executable = dir.join("program");
    for code in cases {
        let program = rill::parser::parse(code).unwrap();
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
