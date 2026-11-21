use std::{fs, process::Command};

#[test]
fn explicit_reload_context_agrees_across_backends() {
    let directory =
        std::env::temp_dir().join(format!("rill-reload-context-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("main.rill");
    fs::write(&entry, include_str!("../examples/reload/main.rill")).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    assert_eq!(rill::interpreter::run(&program).unwrap().to_string(), "13");
    let c = directory.join("program.c");
    fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
    let binary = directory.join("program");
    for level in ["-O0", "-O2"] {
        let output = Command::new("cc")
            .args(["-std=c11", level])
            .arg(&c)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"13\n");
    }
    fs::remove_dir_all(directory).unwrap();
}
