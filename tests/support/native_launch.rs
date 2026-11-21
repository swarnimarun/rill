//! Bounded C-fixture execution. macOS launch scans precede the main deadline.
use std::{
    io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
const WRAPPER: &str = r#"#undef main
#include <fcntl.h>
#include <stdlib.h>
#include <unistd.h>
extern int rill_test_program_main(void);
int main(void) {
    const char *path = getenv("RILL_TEST_MAIN_STARTED");
    if (path == NULL) _exit(125);
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0 || write(fd, "R", 1) != 1 || close(fd) != 0) _exit(125);
    return rill_test_program_main();
}
"#;

#[cfg(target_os = "macos")]
fn prepare(command: &mut Command, directory: &Path) -> io::Result<Option<PathBuf>> {
    let args: Vec<_> = command.get_args().map(|arg| arg.to_owned()).collect();
    if args.iter().any(|arg| arg == "-std=c11") {
        if let Some(output) = args.windows(2).find(|pair| pair[0] == "-o") {
            let binary = Path::new(&output[1]);
            if binary.parent() == Some(directory) {
                let wrapper = directory.join("rill-test-launch.c");
                if !wrapper.exists() {
                    std::fs::write(&wrapper, WRAPPER)?;
                }
                command.arg("-Dmain=rill_test_program_main").arg(wrapper);
            }
        }
        return Ok(None);
    }
    let binary = Path::new(command.get_program());
    if binary.parent() != Some(directory) || !directory.join("rill-test-launch.c").exists() {
        return Ok(None);
    }
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let marker = binary.with_extension(format!(
        "rill-started-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    command.env("RILL_TEST_MAIN_STARTED", &marker);
    Ok(Some(marker))
}

/// Preserve ordinary subprocess deadlines. Instrumented macOS fixture binaries
/// receive a separate bounded launch allowance; their existing execution limit
/// starts only after the wrapper enters main. A timeout always remains an error.
pub fn status(
    command: &mut Command,
    directory: &Path,
    timeout: Duration,
) -> io::Result<ExitStatus> {
    #[cfg(target_os = "macos")]
    let marker = prepare(command, directory)?;
    #[cfg(not(target_os = "macos"))]
    let marker: Option<PathBuf> = {
        let _ = directory;
        None
    };
    let mut child = command.spawn()?;
    let mut entered = marker.is_none();
    let mut deadline = Instant::now()
        + if entered {
            timeout
        } else {
            Duration::from_secs(60)
        };
    loop {
        if !entered && std::fs::read(marker.as_ref().unwrap()).is_ok_and(|bytes| bytes == b"R") {
            entered = true;
            deadline = Instant::now() + timeout;
        }
        if let Some(status) = child.try_wait()? {
            // A short program can create its marker and exit between the
            // preceding read and try_wait. Recheck after observing its exit.
            if marker.is_some()
                && !entered
                && !std::fs::read(marker.as_ref().unwrap()).is_ok_and(|bytes| bytes == b"R")
            {
                return Err(io::Error::other(
                    "native fixture exited before its main-start handshake",
                ));
            }
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                if entered {
                    "subprocess execution deadline exceeded; timeout is not a language trap"
                } else {
                    "native launch deadline exceeded before main; timeout is not a language trap"
                },
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
