use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill cli modules {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) {
        fs::write(self.0.join(name), text).unwrap();
    }
    fn run(&self, command: &str) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg(command)
            .arg(self.0.join("main.rill"))
            .output()
            .unwrap()
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn all_cli_commands_use_checked_file_graph() {
    let tree = Tree::new();
    tree.write("main.rill", "import math as m; let main = m.double(21)");
    tree.write(
        "math.rill",
        "let private x = x + x\nexport let double x = private(x)",
    );
    let check = tree.run("check");
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let run = tree.run("run");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"42\n");
    let c = tree.run("emit-c");
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    let cfile = tree.0.join("out.c");
    let cexe = tree.0.join("from-c");
    fs::write(&cfile, c.stdout).unwrap();
    let result = Command::new("cc")
        .arg("-std=c11")
        .arg(&cfile)
        .arg("-o")
        .arg(&cexe)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(Command::new(&cexe).output().unwrap().stdout, b"42\n");
    let executable = tree.0.join("built");
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("build")
        .arg(tree.0.join("main.rill"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(Command::new(&executable).output().unwrap().stdout, b"42\n");
}

#[test]
fn all_cli_commands_reject_missing_and_cyclic_graphs() {
    let tree = Tree::new();
    for (main, helper, expected) in [
        ("import missing; let main = 0", None, "E-MODULE-IO"),
        (
            "import a; let main = 0",
            Some("import a; let value = 0"),
            "E-MODULE-CYCLE",
        ),
    ] {
        tree.write("main.rill", main);
        if let Some(text) = helper {
            tree.write("a.rill", text);
        }
        for command in ["check", "run", "emit-c", "build"] {
            let target = tree.0.join("must-not-exist");
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_rill"));
            cmd.arg(command).arg(tree.0.join("main.rill"));
            if command == "build" {
                cmd.arg("-o").arg(&target);
            }
            let result = cmd.output().unwrap();
            assert!(!result.status.success(), "{command}");
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(expected),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(result.stdout.is_empty());
            assert!(!target.exists());
        }
    }
}

#[test]
fn single_file_name_import_still_works() {
    let tree = Tree::new();
    tree.write(
        "main.rill",
        "let import x = x\nlet main = let import = import(42) in import",
    );
    let run = tree.run("run");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"42\n");
}
