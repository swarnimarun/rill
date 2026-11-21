use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn request(exe: &Path, path: &Path, input: &[u8]) -> Output {
    let mut child = Command::new(exe)
        .env("RILL_DB_PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn persistent_queries_and_incremental_replication() {
    let dir = Fixture(std::env::temp_dir().join(format!("rill-db-{}", std::process::id())));
    fs::create_dir(&dir.0).unwrap();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/embedded-db");
    let object = dir.0.join("adapter.o");
    success(
        Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-c"])
            .arg(example.join("file.c"))
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    for level in ["0", "3"] {
        let exe = dir
            .0
            .join(format!("db{level}{}", std::env::consts::EXE_SUFFIX));
        success(
            Command::new(env!("CARGO_BIN_EXE_rill"))
                .arg("build")
                .arg(example.join("main.rill"))
                .arg("-o")
                .arg(&exe)
                .args(["--opt-level", level, "--link-arg"])
                .arg(&object)
                .output()
                .unwrap(),
        );
        let primary = dir.0.join(format!("primary{level}"));
        let replica = dir.0.join(format!("replica{level}"));
        assert_eq!(
            success(request(
                &exe,
                &primary,
                b"get 7\nput 7 -2147483648\nget 7\nput -1 8\n"
            )),
            b"missing\n-2147483648\n"
        );
        let first = success(request(&exe, &primary, b"changes 0\n"));
        assert_eq!(first, b"apply 1 7 -2147483648\napply 2 -1 8\n");
        success(request(&exe, &replica, &first));
        success(request(&exe, &primary, b"put 7 2147483647\n"));
        let delta = success(request(&exe, &primary, b"changes 2\n"));
        assert_eq!(delta, b"apply 3 7 2147483647\n");
        success(request(&exe, &replica, &delta));
        success(request(&exe, &replica, &delta));
        success(request(&exe, &replica, &first)); // replay old revisions too
        let bytes = fs::read(&primary).unwrap();
        assert_eq!(fs::read(&replica).unwrap(), bytes);
        assert_eq!(
            success(request(
                &exe,
                &replica,
                b"head\nget 7\nget -1\ncount_ge 0 8\ncount_ge 2 8\nchanges 3\n"
            )),
            b"3\n2147483647\n8\n2\n1\n"
        );
        for (command, error) in [
            ("apply 2 -1 9", "conflict"),
            ("apply 2 12 8", "conflict"),
            ("apply 5 0 0", "gap"),
            ("apply 0 0 0", "positive"),
            ("changes 4", "cursor"),
            ("count_ge -1 0", "cursor"),
            ("put 1 2147483648", "i32"),
        ] {
            let output = request(&exe, &replica, command.as_bytes());
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains(error));
            assert_eq!(fs::read(&replica).unwrap(), bytes);
        }
        for invalid in [&b""[..], b"RDB1x", b"junk"] {
            fs::write(&replica, invalid).unwrap();
            assert!(!request(&exe, &replica, b"get 7\n").status.success());
            assert_eq!(fs::read(&replica).unwrap(), invalid);
        }
        let large = dir.0.join(format!("large{level}"));
        let mut writer = BufWriter::new(fs::File::create(&large).unwrap());
        writer.write_all(b"RDB1").unwrap();
        for key in 1_i32..=100_000 {
            writer.write_all(&key.to_le_bytes()).unwrap();
            writer.write_all(&(key % 2).to_le_bytes()).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);
        assert_eq!(
            success(request(&exe, &large, b"count_ge 0 1\nchanges 99999\n")),
            b"50000\napply 100000 100000 0\n"
        );
    }
}
