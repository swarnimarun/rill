use std::{fs, path::Path, process::Command};

#[test]
fn guide_examples_match_interpreter_and_optimized_c() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let guide = fs::read_to_string(root.join("docs/rust-to-rill.md")).unwrap();
    let temporary = std::env::temp_dir().join(format!("rill-migration-{}", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    struct Cleanup<'a>(&'a Path);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0);
        }
    }
    let _cleanup = Cleanup(&temporary);
    for name in ["basics", "control-flow", "results", "ownership"] {
        let path = root.join(format!("examples/rust-migration/{name}.rill"));
        let source = fs::read_to_string(&path).unwrap();
        assert!(guide.contains(source.trim()), "guide drift: {name}");
        let program = rill::modules::compile::load_program(&path).unwrap();
        rill::check::check(&program).unwrap();
        assert_eq!(
            rill::interpreter::run(&program).unwrap().to_string(),
            "42",
            "{name}"
        );
        let c = temporary.join(format!("{name}.c"));
        let executable = temporary.join(name);
        fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
        for optimization in ["-O0", "-O3"] {
            let build = Command::new("cc")
                .args(["-std=c11", optimization])
                .arg(&c)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                build.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let run = Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(run.stdout, b"42\n", "{name} {optimization}");
        }
    }
}
