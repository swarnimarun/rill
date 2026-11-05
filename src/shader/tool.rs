use std::{
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use super::{error, read_bounded};

const MAX_LOG: usize = 2 * 1024 * 1024;
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn resolve(name: &str) -> Result<OsString, String> {
    let variables: &[&str] = match name {
        "slangc" => &["RILL_SLANGC", "SLANGC"],
        "glslangValidator" => &["RILL_GLSLANG_VALIDATOR"],
        "spirv-val" => &["RILL_SPIRV_VAL"],
        _ => return Err(error("TOOL", "unknown shader tool")),
    };
    for variable in variables {
        if let Some(path) = std::env::var_os(variable) {
            if path.is_empty() {
                return Err(error("TOOL", format!("{variable} must name an executable")));
            }
            let selected = Path::new(&path);
            if selected.is_absolute() || selected.components().count() > 1 {
                return fs::canonicalize(selected)
                    .map(PathBuf::into_os_string)
                    .map_err(|e| {
                        error(
                            "TOOL",
                            format!(
                                "cannot run selected {variable} executable {}: {e}",
                                selected.display()
                            ),
                        )
                    });
            }
            return Ok(path);
        }
    }
    if let Some(path) = rill::toolchain::tool(name)? {
        return Ok(path.into_os_string());
    }
    Ok(OsString::from(name))
}

// Tool arguments come exclusively from typed recipes; no shell interprets source
// paths, entry names or environment-selected executable names.
pub(super) fn run(
    executable: &OsStr,
    args: &[OsString],
    cwd: &Path,
    label: &str,
) -> Result<String, String> {
    let stdout = cwd.join(".tool.stdout");
    let stderr = cwd.join(".tool.stderr");
    let out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stdout)
        .map_err(|e| error("TOOL", e.to_string()))?;
    let err = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stderr)
        .map_err(|e| error("TOOL", e.to_string()))?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    rill::toolchain::configure_runtime(&mut command)?;
    let signals = Signals::new()?;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            remove_logs(&stdout, &stderr);
            return Err(error("TOOL", format!("{label}: cannot run {}: {e}; install the tool or select it with its RILL_* environment variable", executable.to_string_lossy())));
        }
    };
    let start = Instant::now();
    let result = loop {
        let over_limit = [&stdout, &stderr].iter().any(|p| {
            fs::metadata(p)
                .map(|m| m.len() > MAX_LOG as u64)
                .unwrap_or(true)
        });
        if over_limit || start.elapsed() > TOOL_TIMEOUT || signals.interrupted() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let _ = child.wait();
            break Err(error(
                "TOOL",
                format!(
                    "{label}: {}",
                    if over_limit {
                        "tool output exceeded 2 MiB"
                    } else if signals.interrupted() {
                        "cancelled"
                    } else {
                        "tool exceeded the 60 second timeout"
                    }
                ),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let log = [
                    read_bounded(&stdout, MAX_LOG),
                    read_bounded(&stderr, MAX_LOG),
                ]
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
                let log = log
                    .into_iter()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .collect::<Vec<_>>()
                    .join("");
                break if status.success() {
                    Ok(log)
                } else {
                    Err(error(
                        "COMPILE",
                        format!(
                            "{label}: {} exited with {status}\n{log}",
                            executable.to_string_lossy()
                        ),
                    ))
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(error("TOOL", format!("{label}: {e}")));
            }
        }
    };
    remove_logs(&stdout, &stderr);
    result
}

fn remove_logs(stdout: &PathBuf, stderr: &PathBuf) {
    let _ = fs::remove_file(stdout);
    let _ = fs::remove_file(stderr);
}

#[cfg(unix)]
static SIGNAL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
#[cfg(unix)]
static SIGNAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(unix)]
extern "C" fn interrupted(signal: i32) {
    SIGNAL.store(signal, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(unix)]
struct Signals {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Vec<(i32, libc::sigaction)>,
}
#[cfg(unix)]
impl Signals {
    fn new() -> Result<Self, String> {
        let lock = SIGNAL_LOCK
            .lock()
            .map_err(|_| error("TOOL", "shader tool signal lock was poisoned"))?;
        SIGNAL.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut guard = Self {
            _lock: lock,
            previous: Vec::new(),
        };
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = interrupted as *const () as usize;
            unsafe {
                libc::sigemptyset(&mut action.sa_mask);
            }
            if unsafe { libc::sigaction(signal, &action, &mut old) } != 0 {
                return Err(error("TOOL", std::io::Error::last_os_error().to_string()));
            }
            guard.previous.push((signal, old));
        }
        Ok(guard)
    }
    fn interrupted(&self) -> bool {
        SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0
    }
}
#[cfg(unix)]
impl Drop for Signals {
    fn drop(&mut self) {
        for (signal, action) in &self.previous {
            unsafe {
                libc::sigaction(*signal, action, std::ptr::null_mut());
            }
        }
    }
}
#[cfg(not(unix))]
struct Signals;
#[cfg(not(unix))]
impl Signals {
    fn new() -> Result<Self, String> {
        Ok(Self)
    }
    fn interrupted(&self) -> bool {
        false
    }
}
