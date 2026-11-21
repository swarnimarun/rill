use serde_json::Value;
use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(crate) const COMMAND: &str = "__rill-playground-worker";
pub(super) const MAX_REQUEST: usize = 448 * 1024;
const MAX_RESPONSE: usize = 2 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);

pub(crate) fn run() -> Result<(), String> {
    // This process only inspects in-memory source. It never launches another tool.
    #[cfg(target_os = "linux")]
    for (kind, cap) in [(libc::RLIMIT_AS, 512 * 1024 * 1024), (libc::RLIMIT_CORE, 0)] {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: the OS writes its limit pair into an initialized stack value.
        if unsafe { libc::getrlimit(kind, &mut limit) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        limit.rlim_cur = cap.min(limit.rlim_cur);
        limit.rlim_max = cap.min(limit.rlim_max);
        // SAFETY: both values are valid limits and are at most the inherited limits.
        if unsafe { libc::setrlimit(kind, &limit) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    let mut input = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_REQUEST as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    if input.len() > MAX_REQUEST {
        return Err("playground request too large".into());
    }
    let request: Value = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
    serde_json::to_writer(io::stdout().lock(), &rill::explorer::inspect(&request))
        .map_err(|e| e.to_string())
}

pub(super) fn execute(request: &[u8]) -> Result<Vec<u8>, String> {
    // On Linux the procfs entry still points to this exact build after Cargo
    // replaces target/debug/rill. A running site and its workers stay in sync.
    #[cfg(target_os = "linux")]
    let exe = std::path::Path::new("/proc/self/exe");
    #[cfg(not(target_os = "linux"))]
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = Command::new(exe)
        .arg(COMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(request));
        let reader = scope.spawn(move || {
            let mut bytes = Vec::new();
            output
                .take(MAX_RESPONSE as u64 + 1)
                .read_to_end(&mut bytes)?;
            Ok::<_, io::Error>(bytes)
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < DEADLINE => {
                    thread::sleep(Duration::from_millis(10))
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(result.err().map_or_else(
                        || "E-PLAYGROUND-LIMIT: evaluation exceeded five seconds".into(),
                        |e| e.to_string(),
                    ));
                }
            }
        };
        let bytes = reader
            .join()
            .map_err(|_| "compiler output reader stopped")?
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_RESPONSE {
            return Err("E-PLAYGROUND-LIMIT: compiler output too large".into());
        }
        if !status.success() {
            return Err(format!("E-PLAYGROUND-WORKER: compiler stopped ({status}). Reduce the program's memory or computation."));
        }
        writer
            .join()
            .map_err(|_| "compiler input writer stopped")?
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    })
}
