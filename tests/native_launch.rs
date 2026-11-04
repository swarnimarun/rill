#[path = "support/native_launch.rs"]
mod native_launch;

use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rill-native-launch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let c = dir.join("main.c");
        fs::write(&c, source).unwrap();
        let status = native_launch::status(
            Command::new("cc")
                .args(["-std=c11", "-pedantic-errors", "-O0"])
                .arg(&c)
                .arg("-o")
                .arg(dir.join("program")),
            &dir,
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(status.success());
        assert_eq!(
            fs::read_to_string(c).unwrap(),
            source,
            "test instrumentation must preserve emitted C bytes"
        );
        Self(dir)
    }
    fn run(&self, timeout: Duration) -> std::io::Result<std::process::ExitStatus> {
        native_launch::status(
            Command::new(self.0.join("program"))
                .stdout(fs::File::create(self.0.join("stdout")).unwrap())
                .stderr(fs::File::create(self.0.join("stderr")).unwrap())
                .stdin(Stdio::null()),
            &self.0,
            timeout,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn linked_program_keeps_exact_result_and_trap_status() {
    let result = Fixture::new("#include <stdio.h>\nint main(void){puts(\"42\");return 0;}\n");
    for _ in 0..3 {
        assert!(result.run(Duration::from_secs(10)).unwrap().success());
    }
    assert_eq!(fs::read(result.0.join("stdout")).unwrap(), b"42\n");
    assert!(fs::read(result.0.join("stderr")).unwrap().is_empty());
    let trap = Fixture::new(
        "#include <stdio.h>\nint main(void){fputs(\"rill: runtime trap\\n\",stderr);return 1;}\n",
    );
    assert_eq!(trap.run(Duration::from_secs(10)).unwrap().code(), Some(1));
    assert!(fs::read(trap.0.join("stdout")).unwrap().is_empty());
    assert_eq!(
        fs::read(trap.0.join("stderr")).unwrap(),
        b"rill: runtime trap\n"
    );
}

#[test]
fn a_running_infinite_loop_is_still_a_timeout_not_a_trap() {
    let fixture = Fixture::new("int main(void){for(;;){} return 0;}\n");
    assert_eq!(
        fixture.run(Duration::from_millis(100)).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
}

#[cfg(target_os = "macos")]
#[test]
fn startup_delay_does_not_consume_the_main_execution_limit() {
    let fixture=Fixture::new("#include <unistd.h>\n__attribute__((constructor)) static void startup(void){sleep(1);}\nint main(void){return 0;}\n");
    assert!(fixture.run(Duration::from_millis(100)).unwrap().success());
}
