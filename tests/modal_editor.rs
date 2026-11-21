#![cfg(unix)]
use std::{
    fs,
    io::Write,
    os::unix::fs::{symlink, PermissionsExt},
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn edit(binary: &Path, document: &Path, keys: &[u8]) -> Output {
    let mut child = Command::new(binary)
        .env("RILL_EDITOR_FILE", document)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(keys).unwrap();
    child.wait_with_output().unwrap()
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn check_external_replacement(binary: &Path, document: &Path) {
    // The first explicit save is the handshake that the child has loaded the
    // original. Replace its pathname while its command loop remains open.
    fs::write(document, b"old").unwrap();
    let mut child = Command::new(binary)
        .env("RILL_EDITOR_FILE", document)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut keys = child.stdin.take().unwrap();
    keys.write_all(b"$i1\x1bw").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read(document).unwrap() != b"old1" {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("timed out waiting for the editor's first save");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let other = document.with_extension("replacement");
    fs::write(&other, b"outside").unwrap();
    fs::rename(&other, document).unwrap();
    keys.write_all(b"i2\x1bwq").unwrap();
    drop(keys);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Save failed") && error.contains("unsaved"),
        "{error}"
    );
    assert_eq!(fs::read(document).unwrap(), b"outside");
}

#[test]
fn modal_editor_opens_edits_and_saves_real_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/modal-editor");
    let dir = std::env::temp_dir().join(format!("rill-modal-editor-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let object = dir.join("terminal.o");
    success(
        Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic", "-c"])
            .arg(root.join("terminal.c"))
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    for opt in ["0", "3"] {
        let binary = dir.join("editor");
        success(
            Command::new(env!("CARGO_BIN_EXE_rill"))
                .arg("build")
                .arg(root.join("main.rill"))
                .args(["--opt-level", opt, "--link-arg"])
                .arg(&object)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap(),
        );
        let doc = dir.join(format!("text {opt}"));
        // Create, reopen, delete under cursor, move to end, append, backspace, save twice.
        success(edit(&binary, &doc, b"ihello\rworld\x1bwq"));
        assert_eq!(fs::read(&doc).unwrap(), b"hello\nworld");
        fs::set_permissions(&doc, fs::Permissions::from_mode(0o640)).unwrap();
        success(edit(&binary, &doc, b"lx$i!z\x7f\x1bw0ix\x1bwq"));
        assert_eq!(fs::read(&doc).unwrap(), b"xhllo\nworld!");
        assert_eq!(
            fs::metadata(&doc).unwrap().permissions().mode() & 0o777,
            0o640
        );
        success(edit(&binary, &doc, b"llljik\x1bkxwq"));
        assert_eq!(fs::read(&doc).unwrap(), b"xhll\nworkld!");
        success(edit(&binary, &doc, b"ix\x1bqQ"));
        assert_eq!(fs::read(&doc).unwrap(), b"xhll\nworkld!");
        let aborted = edit(&binary, &doc, b"xq");
        assert!(!aborted.status.success());
        assert!(String::from_utf8_lossy(&aborted.stderr).contains("unsaved"));
        assert_eq!(fs::read(&doc).unwrap(), b"xhll\nworkld!");

        check_external_replacement(&binary, &doc);

        let full = vec![b'a'; 256];
        fs::write(&doc, &full).unwrap();
        success(edit(&binary, &doc, b"$ix\x1bwq"));
        assert_eq!(fs::read(&doc).unwrap(), full);
        for bad in [vec![b'z'; 257], vec![b'a', 0], vec![0xc3, 0xa9]] {
            fs::write(&doc, &bad).unwrap();
            assert!(!edit(&binary, &doc, b"").status.success());
            assert_eq!(fs::read(&doc).unwrap(), bad);
        }
        let link = dir.join(format!("link{opt}"));
        symlink(&doc, &link).unwrap();
        assert!(!edit(&binary, &link, b"").status.success());
    }
    fs::remove_dir_all(dir).unwrap();
}
