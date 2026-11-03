//! CLI-only process boundary. The public library importer remains in-process.
//! The worker is the same executable, not a PATH lookup or environment override.
//! This bounds IPC and elapsed work; it is not a memory or security sandbox.
use rill::c_import::{
    configuration,
    selected::{Preparation, Selection},
    Config,
};
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::DirBuilderExt,
        io::AsRawFd,
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicI32, AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(super) const COMMAND: &str = "__rill-import-worker-v1";
const REQUEST_MAGIC: &[u8] = b"RILL-IMPORT-REQUEST\0\x01";
const PREPARE_REQUEST_MAGIC: &[u8] = b"RILL-IMPORT-PREPARE-REQUEST\0\x02";
const RESPONSE_MAGIC: &[u8] = b"RILL-IMPORT-RESPONSE\0\x01";
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_MODULE: usize = 1024 * 1024;
const MAX_RESPONSE: usize = RESPONSE_MAGIC.len() + 4 + MAX_MODULE;
const MAX_DIAGNOSTICS: usize = 64 * 1024;
const MAX_ITEMS: usize = 1024;

fn protocol_error() -> String {
    "E-C-IMPORT: malformed or incompatible import worker protocol".into()
}

struct Encoder(Vec<u8>);
impl Encoder {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_REQUEST.saturating_sub(self.0.len() + 4) {
            return Err("E-C-IMPORT: import worker request exceeds 1 MiB".into());
        }
        self.0
            .extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    fn list<'a>(&mut self, items: impl ExactSizeIterator<Item = &'a OsStr>) -> Result<(), String> {
        if items.len() > MAX_ITEMS {
            return Err("E-C-IMPORT: import worker request exceeds 1024 items per list".into());
        }
        self.0
            .extend_from_slice(&(items.len() as u32).to_le_bytes());
        for item in items {
            self.bytes(item.as_bytes())?;
        }
        Ok(())
    }
}

struct Decoder<'a>(&'a [u8]);
impl<'a> Decoder<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], String> {
        if size > self.0.len() {
            return Err(protocol_error());
        }
        let (value, remaining) = self.0.split_at(size);
        self.0 = remaining;
        Ok(value)
    }

    fn count(&mut self) -> Result<usize, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize)
    }

    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let size = self.count()?;
        self.take(size)
    }

    fn list(&mut self) -> Result<Vec<OsString>, String> {
        let size = self.count()?;
        if size > MAX_ITEMS {
            return Err(protocol_error());
        }
        (0..size)
            .map(|_| self.bytes().map(|b| OsString::from_vec(b.to_vec())))
            .collect()
    }

    fn strings(&mut self) -> Result<Vec<String>, String> {
        self.list()?
            .into_iter()
            .map(|s| s.into_string().map_err(|_| protocol_error()))
            .collect()
    }

    fn done(self) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(protocol_error())
        }
    }
}

fn encode_request(
    selection: &Selection,
    compiler: &OsStr,
    zig_cc: bool,
) -> Result<Vec<u8>, String> {
    let mut output = Encoder(REQUEST_MAGIC.to_vec());
    for list in [
        &selection.native.includes,
        &selection.native.links,
        &selection.native.packages,
        &selection.functions,
        &selection.constants,
    ] {
        output.list(list.iter().map(OsStr::new))?;
    }
    output.list(selection.c_args.iter().map(OsString::as_os_str))?;
    output.list(selection.resource_dir.iter().map(|p| p.as_os_str()))?;
    output.bytes(compiler.as_bytes())?;
    output.bytes(&[u8::from(zig_cc)])?;
    Ok(output.0)
}

fn decode_request(bytes: &[u8]) -> Result<(Selection, OsString, bool), String> {
    if bytes.len() > MAX_REQUEST {
        return Err("E-C-IMPORT: import worker request exceeds 1 MiB".into());
    }
    let mut input = Decoder(bytes);
    if input.take(REQUEST_MAGIC.len())? != REQUEST_MAGIC {
        return Err(protocol_error());
    }
    let native = Config {
        includes: input.strings()?,
        links: input.strings()?,
        packages: input.strings()?,
    };
    let functions = input.strings()?;
    let constants = input.strings()?;
    let c_args = input.list()?;
    let mut resources = input.list()?;
    if resources.len() > 1 {
        return Err(protocol_error());
    }
    let compiler = OsString::from_vec(input.bytes()?.to_vec());
    let zig_cc = match input.bytes()? {
        [0] => false,
        [1] => true,
        _ => return Err(protocol_error()),
    };
    input.done()?;
    Ok((
        Selection {
            native,
            functions,
            constants,
            c_args,
            resource_dir: resources.pop().map(PathBuf::from),
        },
        compiler,
        zig_cc,
    ))
}

fn decode_response(bytes: &[u8], selection: &Selection) -> Result<String, String> {
    let mut input = Decoder(bytes);
    if input.take(RESPONSE_MAGIC.len())? != RESPONSE_MAGIC {
        return Err(protocol_error());
    }
    let module = input.bytes()?;
    if module.is_empty() || module.len() > MAX_MODULE {
        return Err(protocol_error());
    }
    let module = std::str::from_utf8(module).map_err(|_| protocol_error())?;
    input.done()?;
    validate_response_shape(module, selection)?;
    // IPC bytes are not automatically a checked module. Reject invalid Rill even
    // if a broken worker claims success; target checks run inside that worker.
    rill::modules::compile::check_binding_source(module)
        .map_err(|e| format!("E-C-IMPORT: invalid import worker module: {e}"))?;
    Ok(module.to_owned())
}

fn validate_response_shape(module: &str, selection: &Selection) -> Result<(), String> {
    rill::c_import::selected::shape::validate(module, selection).map_err(|_| protocol_error())
}

static CANCELLED: AtomicI32 = AtomicI32::new(0);
extern "C" fn cancel(signal: libc::c_int) {
    // Async-signal-safe: no allocation, I/O, locking or destruction here.
    CANCELLED.store(signal, Ordering::Relaxed);
}

struct Cancellation(Vec<(libc::c_int, libc::sigaction)>);
impl Cancellation {
    fn install() -> Result<Self, String> {
        CANCELLED.store(0, Ordering::Relaxed);
        let mut guard = Self(Vec::new());
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGCHLD] {
            // SAFETY: zero initializes all fields; sigemptyset initializes the
            // mask, and cancel is a signal handler with the correct C ABI.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
            // SIGCHLD must not auto-reap the leader: WNOWAIT/group ownership
            // depends on retaining its PID until kill-group then wait.
            action.sa_sigaction = if signal == libc::SIGCHLD {
                libc::SIG_DFL
            } else {
                cancel as *const () as usize
            };
            unsafe {
                libc::sigemptyset(&mut action.sa_mask);
            }
            if unsafe { libc::sigaction(signal, &action, &mut old) } != 0 {
                return Err(format!(
                    "E-C-IMPORT: cannot install import cancellation handler: {}",
                    io::Error::last_os_error()
                ));
            }
            guard.0.push((signal, old));
        }
        Ok(guard)
    }
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        for (signal, action) in &self.0 {
            // SAFETY: restore precisely the previous handler and mask.
            unsafe {
                libc::sigaction(*signal, action, std::ptr::null_mut());
            }
        }
    }
}

fn check_cancelled() -> Result<(), String> {
    let signal = CANCELLED.load(Ordering::Relaxed);
    if signal == 0 {
        Ok(())
    } else {
        Err(format!(
            "E-C-IMPORT: import worker cancelled by signal {signal}"
        ))
    }
}

pub(super) fn run() -> Result<(), String> {
    // A crashing frontend must not leave a potentially huge core file outside
    // the parent-owned temporary workspace. Compilers inherit this limit too.
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: a valid rlimit pointer for the current process, with no borrowed data.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err(format!(
            "E-C-IMPORT: cannot disable worker core dumps: {}",
            io::Error::last_os_error()
        ));
    }
    let mut request = Vec::new();
    io::stdin()
        .lock()
        .take((MAX_REQUEST + 1) as u64)
        .read_to_end(&mut request)
        .map_err(|e| format!("E-C-IMPORT: cannot read import worker request: {e}"))?;
    if request.starts_with(PREPARE_REQUEST_MAGIC) {
        let (selection, compiler, zig_cc, declared_links) = decode_prepare_request(&request)?;
        let prepared = rill::c_import::selected::prepare_checked(
            &selection,
            &compiler,
            zig_cc,
            |resolved, common| {
                configuration::validate_c_args(common)?;
                configuration::validate_link_sequence(&resolved.link_args, &declared_links)
            },
        )?;
        let response = configuration::encode_preparation(&prepared, &selection)?;
        io::stdout()
            .lock()
            .write_all(&response)
            .map_err(|e| format!("E-C-IMPORT: cannot write preparation response: {e}"))
    } else {
        let (selection, compiler, zig_cc) = decode_request(&request)?;
        let module = rill::c_import::selected::generate(&selection, &compiler, zig_cc)?;
        if module.len() > MAX_MODULE {
            return Err("E-C-IMPORT: import worker module exceeds 1 MiB".into());
        }
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(RESPONSE_MAGIC)
            .and_then(|()| stdout.write_all(&(module.len() as u32).to_le_bytes()))
            .and_then(|()| stdout.write_all(module.as_bytes()))
            .map_err(|e| format!("E-C-IMPORT: cannot write import worker response: {e}"))
    }
}

pub(super) fn generate(
    selection: &Selection,
    compiler: &OsStr,
    zig_cc: bool,
    timeout: Duration,
) -> Result<String, String> {
    let _cancellation = Cancellation::install()?;
    let executable = std::env::current_exe()
        .map_err(|e| format!("E-C-IMPORT: cannot locate import worker executable: {e}"))?;
    let request = encode_request(selection, compiler, zig_cc)?;
    let response = supervise(&executable, &request, timeout)?;
    let module = decode_response(&response, selection)?;
    check_cancelled()?;
    Ok(module)
}

fn encode_prepare_request(
    selection: &Selection,
    compiler: &OsStr,
    zig_cc: bool,
    declared_links: &[OsString],
) -> Result<Vec<u8>, String> {
    configuration::validate_selection(selection)?;
    configuration::validate_link_args(declared_links)?;
    if !Path::new(compiler).is_absolute() || compiler.as_bytes().contains(&0) {
        return Err("E-C-CONFIG: preparation requires an absolute explicit compiler".into());
    }
    let legacy = encode_request(selection, compiler, zig_cc)?;
    let mut output = Encoder(PREPARE_REQUEST_MAGIC.to_vec());
    output.bytes(&legacy)?;
    output.list(declared_links.iter().map(OsString::as_os_str))?;
    if output.0.len() > MAX_REQUEST {
        return Err("E-C-CONFIG: preparation request exceeds 1 MiB".into());
    }
    Ok(output.0)
}
fn decode_prepare_request(
    bytes: &[u8],
) -> Result<(Selection, OsString, bool, Vec<OsString>), String> {
    if bytes.len() > MAX_REQUEST {
        return Err(protocol_error());
    }
    let mut input = Decoder(bytes);
    if input.take(PREPARE_REQUEST_MAGIC.len())? != PREPARE_REQUEST_MAGIC {
        return Err(protocol_error());
    }
    let (selection, compiler, zig_cc) = decode_request(input.bytes()?)?;
    let links = input.list()?;
    input.done()?;
    // Apply identical bounds/grammar in both directions before invoking tools.
    encode_prepare_request(&selection, &compiler, zig_cc, &links)?;
    Ok((selection, compiler, zig_cc, links))
}
pub(super) fn prepare(
    selection: &Selection,
    compiler: &OsStr,
    zig_cc: bool,
    declared_links: &[OsString],
    directory: &Path,
    timeout: Duration,
) -> Result<Preparation, String> {
    let _cancellation = Cancellation::install()?;
    let executable = std::env::current_exe()
        .map_err(|e| format!("E-C-IMPORT: cannot locate import worker executable: {e}"))?;
    let request = encode_prepare_request(selection, compiler, zig_cc, declared_links)?;
    let response = supervise_in(
        &executable,
        &request,
        timeout,
        Some(directory),
        configuration::MAX_ARTIFACT,
    )?;
    let prepared = configuration::decode_preparation(&response, selection)?;
    configuration::validate_link_sequence(&prepared.resolved.link_args, declared_links)?;
    validate_response_shape(&prepared.module, selection)?;
    rill::modules::compile::check_binding_source(&prepared.module)
        .map_err(|e| format!("E-C-IMPORT: invalid prepared binding module: {e}"))?;
    check_cancelled()?;
    Ok(prepared)
}

struct Workspace(PathBuf);
impl Workspace {
    fn create() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let root = std::env::temp_dir();
            let root = if root.is_absolute() {
                root
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("cannot resolve temporary directory: {e}"))?
                    .join(root)
            };
            let path = root.join(format!(
                "rill-import-worker-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!(
                        "E-C-IMPORT: cannot create import worker directory: {e}"
                    ))
                }
            }
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Process(Option<Child>);
impl Process {
    fn exited(&mut self) -> Result<bool, String> {
        // Keep the group leader unreaped until group cleanup. Its PID cannot be
        // reused for an unrelated group, even after a worker crash/early exit.
        // SAFETY: zero is valid for siginfo_t; waitid writes to this live value.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.0.as_ref().unwrap().id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            if error.raw_os_error() == Some(libc::ECHILD) {
                // No longer own this PID. Never signal a possibly reused group.
                // The CLI guard prevents auto-reaping; this also fails safely if
                // an unexpected outside reaper violates that invariant.
                self.0.take();
            }
            return Err(format!("E-C-IMPORT: cannot inspect import worker: {error}"));
        }
        // SAFETY: WEXITED initializes the SIGCHLD fields on an exited process;
        // a zero-initialized si_pid remains zero when no child is ready.
        Ok(unsafe { info.si_pid() } != 0)
    }

    fn kill_group(&self) {
        if let Some(child) = &self.0 {
            // SAFETY: the child created this group via process_group(0). It has
            // not been reaped, so this PID/group number cannot have been reused.
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
        }
    }

    fn finish(&mut self) -> io::Result<ExitStatus> {
        self.kill_group();
        let mut child = self.0.take().unwrap();
        // Also target the child if a faulty tool changed its own process group.
        let _ = child.kill();
        child.wait()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.is_some() {
            let _ = self.finish();
        }
    }
}

fn nonblocking(pipe: &impl AsRawFd) -> Result<(), String> {
    let fd = pipe.as_raw_fd();
    // SAFETY: the pipe owns this open fd throughout both fcntl operations.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(format!(
            "E-C-IMPORT: cannot configure import worker pipe: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

// Bound both retained bytes and work per scheduling turn, so a continuous flood
// cannot starve deadline checks or the other pipe. No detached blocking threads.
fn read_available(
    input: &mut impl Read,
    bytes: &mut Vec<u8>,
    limit: usize,
    name: &str,
) -> Result<bool, String> {
    let mut buffer = [0; 8192];
    for _ in 0..8 {
        match input.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(count) => {
                if count > limit - bytes.len() {
                    return Err(format!(
                        "E-C-IMPORT: import worker {name} exceeds {limit} bytes"
                    ));
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("E-C-IMPORT: cannot read import worker {name}: {e}")),
        }
    }
    Ok(false)
}

fn supervise(executable: &Path, request: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
    supervise_in(executable, request, timeout, None, MAX_RESPONSE)
}

fn supervise_in(
    executable: &Path,
    request: &[u8],
    timeout: Duration,
    directory: Option<&Path>,
    response_limit: usize,
) -> Result<Vec<u8>, String> {
    if request.len() > MAX_REQUEST {
        return Err("E-C-IMPORT: import worker request exceeds 1 MiB".into());
    }
    let workspace = Workspace::create()?;
    let start = Instant::now();
    let mut command = Command::new(executable);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let child = command
        .arg(COMMAND)
        .env("TMPDIR", &workspace.0)
        .env("TMP", &workspace.0)
        .env("TEMP", &workspace.0)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "E-C-IMPORT: cannot start import worker {}: {e}",
                executable.display()
            )
        })?;
    // Dropped before the workspace, including on pipe setup or protocol errors.
    let mut process = Process(Some(child));
    let child = process.0.as_mut().unwrap();
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    nonblocking(stdin.as_ref().unwrap())?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut written = 0;
    let mut write_error = None;
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let (mut out_done, mut err_done, mut exited) = (false, false, false);
    loop {
        check_cancelled()?;
        if start.elapsed() >= timeout {
            return Err(format!(
                "E-C-IMPORT: import worker timed out after {} ms",
                timeout.as_millis()
            ));
        }
        if let Some(pipe) = &mut stdin {
            let end = request.len().min(written + 8192);
            match pipe.write(&request[written..end]) {
                Ok(0) if written < request.len() => {
                    write_error = Some("worker closed its input".to_owned());
                    stdin = None;
                }
                Ok(count) => {
                    written += count;
                    if written == request.len() {
                        stdin = None;
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => {
                    write_error = Some(e.to_string());
                    stdin = None;
                }
            }
        }
        if !out_done {
            out_done = read_available(&mut stdout, &mut output, response_limit, "output")?;
        }
        if !err_done {
            err_done = read_available(
                &mut stderr,
                &mut diagnostics,
                MAX_DIAGNOSTICS,
                "diagnostics",
            )?;
        }
        if !exited && process.exited()? {
            exited = true;
            process.kill_group();
            stdin = None;
        }
        if exited && out_done && err_done {
            let status = process
                .finish()
                .map_err(|e| format!("E-C-IMPORT: cannot wait for import worker: {e}"))?;
            if !status.success() {
                return Err(format!(
                    "E-C-IMPORT: import worker failed ({status}):\n{}",
                    String::from_utf8_lossy(&diagnostics).trim()
                ));
            }
            if written != request.len() || write_error.is_some() {
                return Err(format!(
                    "E-C-IMPORT: cannot send import worker request: {}",
                    write_error.as_deref().unwrap_or("worker exited early")
                ));
            }
            return Ok(output);
        }
        std::thread::sleep(Duration::from_millis(2).min(timeout.saturating_sub(start.elapsed())));
    }
}

// Executable fixtures share one process-wide descriptor table. A concurrent
// spawn can inherit another test's writable script descriptor until exec,
// even with CLOEXEC, and make that script temporarily fail with ETXTBSY after
// its writer closed in the parent. Serialize fixture writes and all launches
// through cleanup; protocol-only tests do not need this guard. Production never
// writes its own executable and does not use this test-only synchronization.
#[cfg(test)]
static EXECUTABLE_FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn executable_fixture_guard() -> std::sync::MutexGuard<'static, ()> {
    EXECUTABLE_FIXTURES.lock().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const MODULE: &str = "@include(\"api.h\");\nexport extern fn selected() -> i32;\n";
    fn selection() -> Selection {
        Selection {
            native: Config {
                includes: vec!["api.h".into()],
                ..Config::default()
            },
            functions: vec!["selected".into()],
            ..Selection::default()
        }
    }
    fn response(module: &[u8]) -> Vec<u8> {
        let mut bytes = RESPONSE_MAGIC.to_vec();
        bytes.extend_from_slice(&(module.len() as u32).to_le_bytes());
        bytes.extend_from_slice(module);
        bytes
    }
    struct Fixture {
        path: PathBuf,
        _execution: std::sync::MutexGuard<'static, ()>,
    }
    impl Fixture {
        fn new() -> Self {
            let execution = executable_fixture_guard();
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "rill-worker-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self {
                path,
                _execution: execution,
            }
        }
        fn script(&self, body: &str) -> PathBuf {
            let path = self.path.join("worker with spaces");
            // Record the parent-owned workspace before fault injection.
            let script = format!("#!/bin/sh\nprintf '%s' \"$TMPDIR\" > '{}'/workspace\nprintf artifact > \"$TMPDIR/artifact\"\n{body}\n", self.path.display());
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
        fn cleaned(&self) {
            let workspace = fs::read_to_string(self.path.join("workspace")).unwrap();
            assert!(
                !Path::new(&workspace).exists(),
                "left workspace {workspace}"
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn protocol_round_trips_non_utf8_paths_and_argument_order() {
        let mut s = selection();
        s.c_args = vec![
            "-I".into(),
            OsString::from_vec(b"path \xff".to_vec()),
            "-DFLAG=1".into(),
        ];
        s.resource_dir = Some(PathBuf::from(OsString::from_vec(b"resource \xfe".to_vec())));
        let compiler = OsString::from_vec(b"compiler \xfd".to_vec());
        let request = encode_request(&s, &compiler, true).unwrap();
        let (decoded, cc, zig) = decode_request(&request).unwrap();
        assert_eq!(decoded.native, s.native);
        assert_eq!(decoded.functions, s.functions);
        assert_eq!(decoded.c_args, s.c_args);
        assert_eq!(decoded.resource_dir, s.resource_dir);
        assert_eq!(cc, compiler);
        assert!(zig);
    }

    #[test]
    fn preparation_request_is_versioned_bounded_and_exact() {
        let mut selected = selection();
        selected.c_args = vec![
            "-I".into(),
            OsString::from_vec(b"path \xff".to_vec()),
            "-O2".into(),
        ];
        let compiler = OsStr::new("/explicit/compiler");
        let links = vec!["-la".into(), "-lb".into(), "-la".into()];
        let request = encode_prepare_request(&selected, compiler, false, &links).unwrap();
        let (roundtrip, cc, zig, arguments) = decode_prepare_request(&request).unwrap();
        assert_eq!(roundtrip, selected);
        assert_eq!(cc, compiler);
        assert!(!zig);
        assert_eq!(arguments, links);
        for end in 0..request.len() {
            assert!(decode_prepare_request(&request[..end]).is_err(), "{end}");
        }
        let mut extra = request.clone();
        extra.push(0);
        assert!(decode_prepare_request(&extra).is_err());
        let mut changed = request;
        changed[PREPARE_REQUEST_MAGIC.len() - 1] = 1;
        assert!(decode_prepare_request(&changed).is_err());
        assert!(decode_request(&changed).is_err());
        assert!(
            encode_prepare_request(&selected, OsStr::new("relative-cc"), false, &links).is_err()
        );
        assert!(encode_prepare_request(&selected, compiler, false, &["-E".into()]).is_err());
        selected.c_args = vec!["-D_Static_assert(x,y)=".into()];
        assert!(encode_prepare_request(&selected, compiler, false, &[]).is_err());
    }

    #[test]
    fn supervised_preparation_uses_child_cwd_without_changing_parent() {
        let fixture = Fixture::new();
        let original = std::env::current_dir().unwrap();
        let worker = fixture.script("cat >/dev/null\npwd");
        let response = supervise_in(
            &worker,
            b"request",
            Duration::from_secs(2),
            Some(&fixture.path),
            configuration::MAX_ARTIFACT,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(response).unwrap().trim(),
            fixture.path.to_str().unwrap()
        );
        assert_eq!(std::env::current_dir().unwrap(), original);
        fixture.cleaned();
    }

    #[test]
    fn request_rejects_truncation_trailing_bytes_and_resource_excess() {
        let request = encode_request(&selection(), OsStr::new("cc"), false).unwrap();
        for end in 0..request.len() {
            assert!(decode_request(&request[..end]).is_err(), "prefix {end}");
        }
        let mut extra = request.clone();
        extra.push(0);
        assert!(decode_request(&extra).is_err());
        let mut invalid_bool = request;
        *invalid_bool.last_mut().unwrap() = 2;
        assert!(decode_request(&invalid_bool).is_err());
        let mut s = selection();
        s.c_args = vec![OsString::from("x".repeat(MAX_REQUEST))];
        assert!(encode_request(&s, OsStr::new("cc"), false)
            .unwrap_err()
            .contains("1 MiB"));
        s.c_args = vec!["-I.".into(); MAX_ITEMS + 1];
        assert!(encode_request(&s, OsStr::new("cc"), false)
            .unwrap_err()
            .contains("1024 items"));
        let mut bad_count = REQUEST_MAGIC.to_vec();
        bad_count.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_request(&bad_count).is_err());
    }

    #[test]
    fn response_requires_exact_version_frame_utf8_and_generated_subset() {
        let good = response(MODULE.as_bytes());
        assert_eq!(decode_response(&good, &selection()).unwrap(), MODULE);
        for end in 0..good.len() {
            assert!(
                decode_response(&good[..end], &selection()).is_err(),
                "prefix {end}"
            );
        }
        let mut extra = good.clone();
        extra.push(0);
        let mut version = good.clone();
        version[RESPONSE_MAGIC.len() - 1] = 2;
        let mut length = good;
        length[RESPONSE_MAGIC.len()..RESPONSE_MAGIC.len() + 4]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        for bad in [
            extra,
            version,
            length,
            response(b"\xff"),
            response(b""),
            response(b"nonsense"),
            response(b"export extern fn wrong() -> i32;"),
            response(b"fn main() { 0 }"),
        ] {
            assert!(decode_response(&bad, &selection()).is_err());
        }
        for bad in [
            format!("{MODULE}type T = C(i32)\n"),
            format!("{MODULE}export fn f() {{ 0 }}\n"),
            format!("{MODULE}export extern const FLAG: i32; type T=C(i32)\n"),
            format!(
                "export extern fn selected(arg: {}i32{}) -> i32;",
                "ptr<".repeat(10_000),
                ">".repeat(10_000)
            ),
            format!(
                "export fn f() {{ {}0{} }}",
                "(".repeat(10_000),
                ")".repeat(10_000)
            ),
            format!(
                "{MODULE}{}",
                (0usize..5000)
                    .map(|i| format!("type T{i}=C{i}(T{})\n", i.saturating_sub(1)))
                    .collect::<String>()
            ),
        ] {
            assert!(decode_response(&response(bad.as_bytes()), &selection()).is_err());
        }
    }

    #[test]
    fn missing_worker_is_actionable() {
        let fixture = Fixture::new();
        let error = supervise(
            &fixture.path.join("missing worker"),
            b"request",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(
            error.starts_with("E-C-IMPORT: cannot start import worker"),
            "{error}"
        );
    }

    #[test]
    fn blocked_input_and_hanging_worker_are_timed_out_and_cleaned() {
        let fixture = Fixture::new();
        let worker = fixture.script("sleep 30 &\nwait");
        let start = Instant::now();
        let error = supervise(
            &worker,
            &vec![b'x'; MAX_REQUEST],
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(start.elapsed() < Duration::from_secs(3));
        fixture.cleaned();
    }

    #[test]
    fn worker_crash_and_failed_exit_never_accept_partial_or_complete_output() {
        for ending in ["kill -KILL $$", "exit 7"] {
            let fixture = Fixture::new();
            let frame = fixture.path.join("response");
            fs::write(&frame, response(MODULE.as_bytes())).unwrap();
            let worker = fixture.script(&format!(
                "cat >/dev/null\ncat '{}'\nprintf 'fixture failure' >&2\n{ending}",
                frame.display()
            ));
            let error = supervise(&worker, b"request", Duration::from_secs(3)).unwrap_err();
            assert!(error.contains("import worker failed"), "{error}");
            assert!(error.contains("fixture failure"), "{error}");
            fixture.cleaned();
        }
    }

    #[test]
    fn infinite_stdout_and_stderr_floods_are_stopped_at_the_limit() {
        for (redirect, message) in [("", "output exceeds"), (">&2", "diagnostics exceeds")] {
            let fixture = Fixture::new();
            let worker = fixture.script(&format!(
                "cat >/dev/null\nwhile :; do printf '%08192d' 0 {redirect}; done"
            ));
            let error = supervise(&worker, b"request", Duration::from_secs(3)).unwrap_err();
            assert!(error.contains(message), "{error}");
            fixture.cleaned();
        }
    }

    #[test]
    fn exited_worker_cannot_leave_descendants_holding_pipes() {
        let fixture = Fixture::new();
        let survived = fixture.path.join("survived");
        let worker = fixture.script(&format!(
            "cat >/dev/null\n(sleep 1; printf survived > '{}') &\nprintf response\nexit 0",
            survived.display()
        ));
        let output = supervise(&worker, b"request", Duration::from_secs(3)).unwrap();
        assert_eq!(output, b"response");
        fixture.cleaned();
        std::thread::sleep(Duration::from_millis(1200));
        assert!(
            !survived.exists(),
            "worker descendant escaped normal-exit cleanup"
        );
    }

    #[test]
    fn closed_output_pipes_do_not_count_as_success_before_worker_exit() {
        let fixture = Fixture::new();
        let worker = fixture.script("cat >/dev/null\nexec 1>&- 2>&-\nsleep 30 &\nwait");
        let error = supervise(&worker, b"request", Duration::from_millis(200)).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        fixture.cleaned();
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn malformed_successful_workers_never_replace_existing_output() {
        let _execution = executable_fixture_guard();
        let directory = Workspace::create().unwrap();
        let destination = directory.0.join("bindings.rill");
        let worker = directory.0.join("fake worker");
        let frame_path = directory.0.join("frame");
        fs::write(
            &worker,
            format!(
                "#!/bin/sh\ncat >/dev/null\ncat '{}'\n",
                frame_path.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        let selection = Selection {
            native: Config {
                includes: vec!["api.h".into()],
                ..Config::default()
            },
            functions: vec!["selected".into()],
            ..Selection::default()
        };
        for bytes in [
            b"broken framing".as_slice(),
            b"RILL-IMPORT-RESPONSE\0\x01\xff\xff\xff\xff",
            b"RILL-IMPORT-RESPONSE\0\x01\x01\0\0\0\xff",
        ] {
            fs::write(&destination, "sentinel\n").unwrap();
            fs::write(&frame_path, bytes).unwrap();
            let result = supervise(&worker, b"request", Duration::from_secs(3))
                .and_then(|response| decode_response(&response, &selection))
                .and_then(|module| rill::c_import::selected::write_atomic(&destination, &module));
            assert!(result.is_err());
            assert_eq!(fs::read_to_string(&destination).unwrap(), "sentinel\n");
            assert!(fs::read_dir(&directory.0).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".rill-import-")));
        }
    }
}
