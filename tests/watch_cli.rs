use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill-watch-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Watcher(Child);
impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn once_runs_and_reports_build_and_runtime_errors() {
    let tree = Tree::new();
    let entry = tree.0.join("main.rill");
    for (source, expected, success) in [
        ("fn main() { 42 }", "42\n", true),
        ("fn main() { true + 1 }", "", false),
        ("fn main() { 1 / 0 }", "", false),
    ] {
        fs::write(&entry, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("watch")
            .arg(&entry)
            .args(["--once", "--opt-level", "0"])
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
    for options in [
        vec!["--interval-ms", "0"],
        vec!["--interpret"],
        vec!["--once", "--once"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("watch")
            .arg(&entry)
            .args(options)
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
}

#[test]
fn watches_import_changes_recovers_after_errors_and_ignores_build_output() {
    let tree = Tree::new();
    let entry = tree.0.join("main.rill");
    let module = tree.0.join("value.rill");
    fs::write(&entry, "import value; fn main() { value.answer() }").unwrap();
    fs::write(&module, "export fn answer() { 1 }").unwrap();
    let mut watcher = Watcher(
        Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("watch")
            .arg(&entry)
            .args(["--interval-ms", "20", "--opt-level", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = watcher.0.stdout.take().unwrap();
    let stderr = watcher.0.stderr.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let errors = sender.clone();
    let output_thread = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send((true, line.unwrap())).is_err() {
                break;
            }
        }
    });
    let error_thread = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            if errors.send((false, line.unwrap())).is_err() {
                break;
            }
        }
    });
    let wait_for = |stdout: bool, expected: &str| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let (is_output, line) = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|e| panic!("waiting for {expected:?}: {e}"));
            if is_output == stdout && line.contains(expected) {
                break;
            }
        }
    };
    wait_for(true, "1");
    fs::create_dir(tree.0.join("build")).unwrap();
    fs::write(tree.0.join("build/generated.c"), "not valid C").unwrap();
    fs::write(&module, "export fn answer() { true + 1 }").unwrap();
    wait_for(false, "E-WATCH-BUILD");
    fs::write(&module, "export fn answer() { 2 }").unwrap();
    wait_for(true, "2");
    watcher.0.kill().unwrap();
    watcher.0.wait().unwrap();
    output_thread.join().unwrap();
    error_thread.join().unwrap();
}

#[cfg(unix)]
#[test]
fn failed_build_retains_live_program_and_success_or_termination_stops_it() {
    let tree = Tree::new();
    let entry = tree.0.join("main.rill");
    let native = tree.0.join("wait.c");
    fs::write(
        &native,
        r#"
#include <stdio.h>
#include <unistd.h>
int rill_watch_wait(void) {
    printf("PID=%ld\n", (long)getpid());
    fflush(stdout);
    for (;;) sleep(1);
}
"#,
    )
    .unwrap();
    let waiting_source = "extern fn rill_watch_wait() -> i32; fn main() { rill_watch_wait() }";
    fs::write(&entry, waiting_source).unwrap();
    fs::write(
        tree.0.join("rill.json"),
        r#"{"version":1,"name":"watch-test","entry":"main.rill"}"#,
    )
    .unwrap();
    let mut watcher = Watcher(
        Command::new(env!("CARGO_BIN_EXE_rill"))
            .args(["project", "watch"])
            .arg(&tree.0)
            .args(["--interval-ms", "20", "--opt-level", "0", "--link-arg"])
            .arg(&native)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = watcher.0.stdout.take().unwrap();
    let stderr = watcher.0.stderr.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let errors = sender.clone();
    let output_thread = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send((true, line.unwrap())).is_err() {
                break;
            }
        }
    });
    let error_thread = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            if errors.send((false, line.unwrap())).is_err() {
                break;
            }
        }
    });
    let wait_for = |stdout: bool, prefix: &str| -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (is_output, line) = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|e| panic!("waiting for {prefix:?}: {e}"));
            if is_output == stdout && line.starts_with(prefix) {
                return line;
            }
        }
    };
    struct NativePid(libc::pid_t);
    impl NativePid {
        fn running(&self) -> bool {
            // SAFETY: signal zero queries existence without modifying the process.
            unsafe { libc::kill(self.0, 0) == 0 }
        }
    }
    impl Drop for NativePid {
        fn drop(&mut self) {
            // SAFETY: this guard refers only to the PID emitted by our fixture.
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
        }
    }
    let pid = NativePid(wait_for(true, "PID=")[4..].parse().unwrap());
    fs::write(&entry, "fn main() { true + 1 }").unwrap();
    wait_for(false, "E-WATCH-BUILD:");
    assert!(pid.running(), "failed candidate stopped the active process");
    fs::write(&entry, "fn main() { 42 }").unwrap();
    wait_for(true, "42");
    assert!(!pid.running(), "old process survived replacement");
    // Disarm the guard once that child has been reaped; its PID can be reused.
    std::mem::forget(pid);
    fs::write(&entry, waiting_source).unwrap();
    let pid = NativePid(wait_for(true, "PID=")[4..].parse().unwrap());
    // SAFETY: signal our still-owned watcher child, which owns its process group.
    assert_eq!(
        unsafe { libc::kill(watcher.0.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while watcher.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "watcher did not handle SIGTERM");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !pid.running(),
        "native process survived watcher termination"
    );
    std::mem::forget(pid);
    output_thread.join().unwrap();
    error_thread.join().unwrap();
}
