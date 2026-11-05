//! Frontend reuse must never reuse a native executable or skip changed inputs.
#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill incremental {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn source(&self, source: &str) -> PathBuf {
        let path = self.0.join("main.rill");
        fs::write(&path, source).unwrap();
        path
    }

    fn command(&self, action: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .current_dir(&self.0)
            .arg(action)
            .arg("main.rill")
            .args(["--cache-dir", "cache", "--cache-status"]);
        command
    }

    fn entries(&self) -> Vec<PathBuf> {
        fs::read_dir(self.0.join("cache/checked-c-v1"))
            .into_iter()
            .flatten()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "cache")
            })
            .collect()
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn status(output: &Output, expected: &str) {
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(&format!("rill cache: {expected}")),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn raw_commands_are_opt_in_and_source_bytes_invalidate_emitted_c() {
    let tree = Tree::new();
    tree.source("fn main() { 42 }");
    let disabled = Command::new(env!("CARGO_BIN_EXE_rill"))
        .current_dir(&tree.0)
        .args(["emit-c", "main.rill", "--cache-status"])
        .output()
        .unwrap();
    success(&disabled);
    status(&disabled, "disabled");
    assert!(!tree.0.join("build").exists());

    let cold = tree.command("emit-c").output().unwrap();
    success(&cold);
    status(&cold, "miss");
    let warm = tree.command("emit-c").output().unwrap();
    success(&warm);
    status(&warm, "hit");
    assert_eq!(cold.stdout, warm.stdout);
    assert_eq!(warm.stdout, disabled.stdout);

    tree.source("// same program, different snapshot\nfn main() { 42 }");
    let changed = tree.command("emit-c").output().unwrap();
    success(&changed);
    status(&changed, "miss");
    assert_eq!(cold.stdout, changed.stdout);
    assert_eq!(tree.entries().len(), 2);
}

#[test]
fn imported_source_and_unused_declarations_are_rechecked() {
    let tree = Tree::new();
    tree.source("import answer; fn main() { answer.value() }");
    let imported = tree.0.join("answer.rill");
    fs::write(&imported, "export fn value() { 41 }").unwrap();
    let cold = tree.command("run").output().unwrap();
    success(&cold);
    status(&cold, "miss");
    assert_eq!(cold.stdout, b"41\n");
    let warm = tree.command("run").output().unwrap();
    success(&warm);
    status(&warm, "hit");

    fs::write(&imported, "export fn value() { 42 }").unwrap();
    let changed = tree.command("run").output().unwrap();
    success(&changed);
    status(&changed, "miss");
    assert_eq!(changed.stdout, b"42\n");

    fs::write(
        &imported,
        "export fn value() { 42 } fn invalid_unused() { true + 1 }",
    )
    .unwrap();
    let invalid = tree.command("emit-c").output().unwrap();
    assert!(!invalid.status.success());
    status(&invalid, "miss");
    assert!(invalid.stdout.is_empty());
    assert_eq!(tree.entries().len(), 2);

    fs::remove_file(imported).unwrap();
    let removed = tree.command("run").output().unwrap();
    assert!(!removed.status.success());
    assert!(removed.stdout.is_empty());
}

#[test]
fn corrupted_entries_fall_back_to_a_checked_build() {
    let tree = Tree::new();
    tree.source("fn main() { 42 }");
    let cold = tree.command("emit-c").output().unwrap();
    success(&cold);
    let entries = tree.entries();
    assert_eq!(entries.len(), 1);
    let mut corrupted = fs::read(&entries[0]).unwrap();
    *corrupted.last_mut().unwrap() ^= 1;
    fs::write(&entries[0], corrupted).unwrap();
    let repaired = tree.command("emit-c").output().unwrap();
    success(&repaired);
    status(&repaired, "miss");
    assert_eq!(repaired.stdout, cold.stdout);
    let warm = tree.command("emit-c").output().unwrap();
    success(&warm);
    status(&warm, "hit");
}

#[test]
fn hits_always_compile_with_current_flags_headers_and_link_inputs() {
    let tree = Tree::new();
    tree.source("fn main() { 42 }");
    let wrapper = tree.0.join("compiler wrapper");
    fs::write(
        &wrapper,
        "#!/bin/sh\nprintf 'compile\\n' >> \"$RILL_TEST_COUNT\"\nexec cc \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let count = tree.0.join("compile count");
    let header = tree.0.join("extra.h");
    fs::write(&header, "/* current external header */\n").unwrap();
    let executable = tree.0.join("program");
    let build = || {
        let mut command = tree.command("build");
        command
            .arg("-o")
            .arg(&executable)
            .arg("--cc")
            .arg(&wrapper)
            .args(["--opt-level", "0", "--c-arg", "-include", "--c-arg"])
            .arg(&header)
            .env("RILL_TEST_COUNT", &count);
        command
    };
    let cold = build().output().unwrap();
    success(&cold);
    status(&cold, "miss");
    fs::remove_file(&executable).unwrap();
    let warm = build().output().unwrap();
    success(&warm);
    status(&warm, "hit");
    assert_eq!(fs::read_to_string(&count).unwrap().lines().count(), 2);
    assert_eq!(Command::new(&executable).output().unwrap().stdout, b"42\n");

    fs::write(&header, "#error changed header must be compiled\n").unwrap();
    let changed_header = build().output().unwrap();
    assert!(!changed_header.status.success());
    status(&changed_header, "hit");
    assert!(String::from_utf8_lossy(&changed_header.stderr).contains("changed header"));
    fs::write(&header, "/* restored */\n").unwrap();
    let changed_link = build()
        .arg("--link-arg")
        .arg(tree.0.join("missing-object.o"))
        .output()
        .unwrap();
    assert!(!changed_link.status.success());
    status(&changed_link, "hit");
    assert_eq!(fs::read_to_string(&count).unwrap().lines().count(), 4);

    let invalid_flags = build().args(["--c-arg", "-E"]).output().unwrap();
    assert!(!invalid_flags.status.success());
    assert!(String::from_utf8_lossy(&invalid_flags.stderr).contains("E-C-IMPORT"));
    assert_eq!(fs::read_to_string(&count).unwrap().lines().count(), 4);
}

#[test]
fn foreign_declarations_and_native_dependencies_are_never_stored() {
    let tree = Tree::new();
    tree.source("extern fn foreign_answer() -> i32; fn main() { 42 }");
    for _ in 0..2 {
        let result = tree.command("emit-c").output().unwrap();
        success(&result);
        status(&result, "miss");
        assert!(tree.entries().is_empty());
    }

    tree.source("@include(\"fixture.h\"); fn main() { 42 }");
    let header = tree.0.join("fixture.h");
    fs::write(&header, "int foreign_answer(void);\n").unwrap();
    let emit = || {
        let mut command = tree.command("emit-c");
        command.arg("--c-arg").arg("-I").arg("--c-arg").arg(&tree.0);
        command
    };
    let valid = emit().output().unwrap();
    success(&valid);
    assert!(tree.entries().is_empty());
    fs::write(header, "#error native header changed\n").unwrap();
    let invalid = emit().output().unwrap();
    assert!(!invalid.status.success());
    status(&invalid, "miss");
    assert!(invalid.stdout.is_empty());
}

#[test]
fn check_and_interpreter_bypass_existing_entries() {
    let tree = Tree::new();
    tree.source("fn main() { 42 }");
    success(&tree.command("emit-c").output().unwrap());
    let check = tree.command("check").output().unwrap();
    success(&check);
    status(&check, "bypass");
    assert!(check.stdout.is_empty());
    let interpret = tree.command("run").arg("--interpret").output().unwrap();
    success(&interpret);
    status(&interpret, "bypass");
    assert_eq!(interpret.stdout, b"42\n");
}

#[test]
fn concurrent_builds_publish_complete_entries() {
    let tree = Tree::new();
    tree.source("fn main() { 42 }");
    let children: Vec<_> = (0..4)
        .map(|_| {
            tree.command("emit-c")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let results: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    for result in &results {
        success(result);
        assert_eq!(result.stdout, results[0].stdout);
    }
    assert_eq!(tree.entries().len(), 1);
    let warm = tree.command("emit-c").output().unwrap();
    success(&warm);
    status(&warm, "hit");
}

#[test]
fn project_build_run_and_watch_share_the_default_cache() {
    let tree = Tree::new();
    let scaffold = Command::new(env!("CARGO_BIN_EXE_rill"))
        .current_dir(&tree.0)
        .args(["new", "app"])
        .output()
        .unwrap();
    success(&scaffold);
    let project = |action| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .current_dir(&tree.0)
            .args(["project", action, "app", "--cache-status"]);
        command
    };
    let cold = project("build").output().unwrap();
    success(&cold);
    status(&cold, "miss");
    assert!(tree.0.join("app/build/.rill-cache/checked-c-v1").is_dir());
    assert!(!tree.0.join("app/src/build").exists());
    let warm = project("run").output().unwrap();
    success(&warm);
    status(&warm, "hit");
    assert_eq!(warm.stdout, b"42\n");
    let watch = project("watch").arg("--once").output().unwrap();
    success(&watch);
    status(&watch, "hit");
    assert_eq!(watch.stdout, b"42\n");
    let disabled = project("build").arg("--no-incremental").output().unwrap();
    success(&disabled);
    status(&disabled, "disabled");

    for path in ["../escape", "src/main.rill", "build/app", "build"] {
        let invalid = project("build")
            .args(["--cache-dir", path])
            .output()
            .unwrap();
        assert!(!invalid.status.success(), "accepted {path}");
    }
    std::os::unix::fs::symlink(&tree.0, tree.0.join("app/link")).unwrap();
    let symlink = project("build")
        .args(["--cache-dir", "link/cache"])
        .output()
        .unwrap();
    assert!(!symlink.status.success());
    assert!(!tree.0.join("cache").exists());
}

#[test]
fn incremental_options_reject_ambiguous_combinations() {
    let tree = Tree::new();
    tree.source("fn main() { 42 }");
    for args in [
        vec!["--incremental", "--no-incremental"],
        vec!["--incremental", "--incremental"],
        vec!["--cache-dir", "cache", "--no-incremental"],
        vec!["--cache-dir", "first", "--cache-dir", "second"],
        vec!["--cache-dir"],
        vec!["--cache-dir", ""],
        vec!["--cache-status", "--cache-status"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_rill"))
            .current_dir(&tree.0)
            .args(["emit-c", "main.rill"])
            .args(&args)
            .output()
            .unwrap();
        assert!(!result.status.success(), "accepted {args:?}");
        assert!(result.stdout.is_empty());
    }
    assert!(!tree.0.join("cache").exists());
}

#[test]
fn cache_directories_cannot_overlap_executable_outputs() {
    let tree = Tree::new();
    tree.source("fn main() { 42 }");
    for args in [
        vec!["--incremental", "-o", "build"],
        vec!["--cache-dir", "artifact/cache", "-o", "artifact"],
        vec!["--cache-dir", "cache", "-o", "cache/program"],
    ] {
        let invalid = Command::new(env!("CARGO_BIN_EXE_rill"))
            .current_dir(&tree.0)
            .args(["build", "main.rill"])
            .args(&args)
            .output()
            .unwrap();
        assert!(!invalid.status.success(), "accepted {args:?}");
        assert!(String::from_utf8_lossy(&invalid.stderr).contains("E-CACHE-PATH"));
    }
    for path in ["build", "artifact", "cache"] {
        assert!(!tree.0.join(path).exists());
    }

    fs::create_dir(tree.0.join("cache")).unwrap();
    std::os::unix::fs::symlink(tree.0.join("cache"), tree.0.join("alias")).unwrap();
    let alias = tree
        .command("build")
        .args(["-o", "alias/program"])
        .output()
        .unwrap();
    assert!(!alias.status.success());
    assert!(String::from_utf8_lossy(&alias.stderr).contains("E-CACHE-PATH"));
    assert!(fs::read_dir(tree.0.join("cache")).unwrap().next().is_none());
}

#[test]
fn default_project_cache_preserves_a_manifest_output_named_build() {
    let tree = Tree::new();
    success(
        &Command::new(env!("CARGO_BIN_EXE_rill"))
            .current_dir(&tree.0)
            .args(["new", "app"])
            .output()
            .unwrap(),
    );
    fs::write(
        tree.0.join("app/rill.json"),
        r#"{"version":1,"name":"app","entry":"src/main.rill","output":"build"}"#,
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_rill"))
        .current_dir(&tree.0)
        .args(["project", "build", "app", "--cache-status"])
        .output()
        .unwrap();
    success(&result);
    status(&result, "disabled");
    assert!(tree.0.join("app/build").is_file());
    assert_eq!(
        Command::new(tree.0.join("app/build"))
            .output()
            .unwrap()
            .stdout,
        b"42\n"
    );
}
