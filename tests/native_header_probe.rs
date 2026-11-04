#![cfg(all(feature = "c-import", unix))]

use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

static EXECUTABLES: Mutex<()> = Mutex::new(());

struct Fixture {
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let guard = EXECUTABLES
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root =
            std::env::temp_dir().join(format!("rill-native-probe-{}-{name}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("api.h"),
            "#include <stdint.h>\nint32_t api_count(void);\n",
        )
        .unwrap();
        fs::write(
            root.join("main.rill"),
            "@include(\"api.h\"); fn main() { api_count() }\n",
        )
        .unwrap();
        Self {
            root,
            _guard: guard,
        }
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, format!("#!/bin/sh\nif test \"$1\" = __rill-fixture-ready; then exit 0; fi\nwrite_pid() {{ printf '%s' \"$2\" > \"$1.tmp\"; mv \"$1.tmp\" \"$1\"; }}\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        #[cfg(target_os = "macos")]
        assert!(Command::new(&path)
            .arg("__rill-fixture-ready")
            .status()
            .unwrap()
            .success());
        path
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .arg("check")
            .arg(self.root.join("main.rill"))
            .args(["--c-arg", "-I", "--c-arg"])
            .arg(&self.root)
            .env("PROBE_PID", self.root.join("probe.pid"))
            .env("DESCENDANT_PID", self.root.join("descendant.pid"));
        command
    }

    fn compiler(&self, probe: &str) -> PathBuf {
        self.script(
            "compiler",
            &format!("if [ \"$1\" = -print-resource-dir ]; then\n{probe}\nfi\nexec cc \"$@\""),
        )
    }

    fn pid(&self, name: &str) -> Option<libc::pid_t> {
        fs::read_to_string(self.root.join(name))
            .ok()?
            .parse()
            .ok()
            .filter(|pid| *pid > 0)
    }

    fn observed(
        &self,
        command: &mut Command,
        markers: &[&str],
        fallback_error: Option<&str>,
    ) -> Running {
        let running = Running::start(command);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if markers.iter().all(|name| self.pid(name).is_some()) {
                return running;
            }
            if running.exited() {
                let output = running.output();
                if let Some(error) = fallback_error {
                    assert!(!output.status.success());
                    assert!(String::from_utf8_lossy(&output.stderr).contains(error));
                }
                panic!(
                    "fixture did not acknowledge discovery: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert!(
                Instant::now() < deadline,
                "fixture startup exceeded deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_gone(&self, name: &str) {
        let pid = self
            .pid(name)
            .unwrap_or_else(|| panic!("{}: missing complete {name}", self.root.display()));
        let deadline = Instant::now() + Duration::from_secs(3);
        while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_ne!(
            unsafe { libc::kill(pid, 0) },
            0,
            "{name} survived the probe"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "Kept native discovery failure artifacts: {}",
                self.root.display()
            );
            return;
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

// Retain the unreaped CLI leader until group cleanup. A broken discovery probe
// must fail this test within a deadline rather than hang the regression suite.
struct Running(Option<Child>);
impl Running {
    fn start(command: &mut Command) -> Self {
        Self(Some(
            command
                .process_group(0)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }

    fn pid(&self) -> libc::pid_t {
        self.0.as_ref().unwrap().id() as libc::pid_t
    }

    fn exited(&self) -> bool {
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.pid() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        assert_eq!(result, 0);
        (unsafe { info.si_pid() }) != 0
    }

    fn output(mut self) -> Output {
        let start = Instant::now();
        while !self.exited() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "native header discovery exceeded the test deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        unsafe { libc::kill(-self.pid(), libc::SIGKILL) };
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn hanging_compiler_discovery_falls_back_and_cleans_ordinary_descendants() {
    let fixture = Fixture::new("hanging");
    let compiler = fixture
        .compiler("  write_pid \"$PROBE_PID\" \"$$\"\n  sleep 60 &\n  write_pid \"$DESCENDANT_PID\" \"$!\"\n  wait\n  exit 1");
    let output = fixture
        .observed(
            fixture.command().arg("--cc").arg(compiler),
            &["probe.pid", "descendant.pid"],
            None,
        )
        .output();
    succeeds(&output);
    fixture.assert_gone("probe.pid");
    fixture.assert_gone("descendant.pid");
}

#[test]
fn compiler_discovery_floods_and_unknown_options_fall_back() {
    for (name, probe) in [
        (
            "stdout",
            "  write_pid \"$PROBE_PID\" \"$$\"\n  while :; do printf '%8192s' x; done",
        ),
        (
            "stderr",
            "  write_pid \"$PROBE_PID\" \"$$\"\n  while :; do printf '%8192s' x >&2; done",
        ),
        ("unknown", "  write_pid \"$PROBE_PID\" \"$$\"\n  exit 1"),
    ] {
        let fixture = Fixture::new(name);
        let compiler = fixture.compiler(probe);
        let output = fixture
            .observed(
                fixture.command().arg("--cc").arg(compiler),
                &["probe.pid"],
                None,
            )
            .output();
        succeeds(&output);
        fixture.assert_gone("probe.pid");
    }
}

#[test]
fn compiler_discovery_cancellation_stops_build_preparation_and_descendants() {
    let fixture = Fixture::new("cancel");
    let compiler = fixture
        .compiler("  write_pid \"$PROBE_PID\" \"$$\"\n  sleep 60 &\n  write_pid \"$DESCENDANT_PID\" \"$!\"\n  wait\n  exit 1");
    let running = fixture.observed(
        fixture.command().arg("--cc").arg(compiler),
        &["probe.pid", "descendant.pid"],
        None,
    );
    unsafe { libc::kill(running.pid(), libc::SIGINT) };
    let output = running.output();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cancelled by signal"),
        "{output:?}"
    );
    fixture.assert_gone("probe.pid");
    fixture.assert_gone("descendant.pid");
}

#[cfg(target_os = "macos")]
#[test]
fn sdk_discovery_has_the_same_deadline_and_descendant_cleanup() {
    let fixture = Fixture::new("sdk");
    fixture.script(
        "xcrun",
        "write_pid \"$PROBE_PID\" \"$$\"\nsleep 60 &\nwrite_pid \"$DESCENDANT_PID\" \"$!\"\nwait",
    );
    let mut paths = vec![fixture.root.clone()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let mut command = fixture.command();
    command
        .env_remove("SDKROOT")
        .env("PATH", std::env::join_paths(paths).unwrap());
    let output = fixture
        .observed(
            &mut command,
            &["probe.pid", "descendant.pid"],
            Some("cannot discover the macOS SDK"),
        )
        .output();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot discover the macOS SDK"),
        "{output:?}"
    );
    fixture.assert_gone("probe.pid");
    fixture.assert_gone("descendant.pid");
}
