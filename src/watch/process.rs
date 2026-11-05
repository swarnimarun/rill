//! Child ownership and cooperative watcher cancellation.
use std::{
    io,
    process::{Child, Command, ExitStatus},
    thread,
    time::Duration,
};

#[cfg(unix)]
use std::{
    os::unix::process::CommandExt,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

#[cfg(unix)]
static CANCELLED: AtomicI32 = AtomicI32::new(0);
#[cfg(unix)]
static INSTALLED: AtomicBool = AtomicBool::new(false);

pub(super) fn cancelled() -> bool {
    #[cfg(unix)]
    return CANCELLED.load(Ordering::Relaxed) != 0;
    #[cfg(not(unix))]
    false
}

#[cfg(unix)]
extern "C" fn cancel(signal: libc::c_int) {
    // Signal-safe: only a lock-free atomic store, with no I/O or allocation.
    CANCELLED.store(signal, Ordering::Relaxed);
}

#[cfg(unix)]
struct Cancellation(Vec<(libc::c_int, libc::sigaction)>);

#[cfg(unix)]
impl Cancellation {
    fn install() -> Result<Self, String> {
        INSTALLED
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .map_err(|_| "E-WATCH: cancellation handler is already installed")?;
        CANCELLED.store(0, Ordering::Relaxed);
        let mut guard = Self(Vec::new());
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGCHLD] {
            // SAFETY: zero initializes these platform-defined structures, and
            // sigemptyset initializes their mask before passing to sigaction.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
            // Retain child PIDs until wait: inherited SIGCHLD=SIG_IGN or
            // SA_NOCLDWAIT would otherwise invalidate process-group ownership.
            action.sa_sigaction = if signal == libc::SIGCHLD {
                libc::SIG_DFL
            } else {
                cancel as *const () as usize
            };
            // SAFETY: both pointers refer to live libc sigaction structures;
            // cancel has the required C ABI and installs no extended handler.
            unsafe {
                libc::sigemptyset(&mut action.sa_mask);
            }
            if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                return Err(format!(
                    "E-WATCH: cannot install cancellation handler: {}",
                    io::Error::last_os_error()
                ));
            }
            guard.0.push((signal, previous));
        }
        Ok(guard)
    }
}

#[cfg(unix)]
impl Drop for Cancellation {
    fn drop(&mut self) {
        for (signal, previous) in self.0.iter().rev() {
            // SAFETY: restore exactly the handler and mask returned by
            // sigaction, after all supervised children have been stopped.
            unsafe {
                libc::sigaction(*signal, previous, std::ptr::null_mut());
            }
        }
        INSTALLED.store(false, Ordering::Relaxed);
    }
}

struct Process(Option<Child>);

impl Process {
    fn start(command: &mut Command) -> Result<Self, String> {
        if cancelled() {
            return Err("E-WATCH: cancelled".into());
        }
        #[cfg(unix)]
        command.process_group(0);
        command
            .spawn()
            .map(|child| Self(Some(child)))
            .map_err(|e| format!("E-WATCH: cannot start process: {e}"))
    }

    fn poll(&mut self) -> Result<Option<ExitStatus>, String> {
        let Some(child) = self.0.as_mut() else {
            return Ok(None);
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                // Never signal this process-group ID after reaping its leader:
                // it may be reused. Background jobs outliving their parent are
                // outside the watcher's supported lifecycle.
                self.0 = None;
                Ok(Some(status))
            }
            Ok(None) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(None),
            Err(error) => {
                #[cfg(unix)]
                if error.raw_os_error() == Some(libc::ECHILD) {
                    // An unexpected external reaper invalidated ownership.
                    self.0 = None;
                }
                Err(format!("E-WATCH: cannot poll process: {error}"))
            }
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        // try_wait first avoids signaling a leader that we already reaped.
        self.poll()?;
        let Some(mut child) = self.0.take() else {
            return Ok(());
        };
        #[cfg(unix)]
        {
            // SAFETY: process_group(0) created this group. Its leader is our
            // unreaped child, so its PID cannot name a reused unrelated group.
            // Kill descendants before reaping the leader.
            let result = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    // Retain ownership so Drop can still attempt cleanup.
                    self.0 = Some(child);
                    return Err(format!("E-WATCH: cannot stop process group: {error}"));
                }
            }
        }
        // Also target a child that changed its own process group. On non-Unix
        // hosts only the immediate process is supervised.
        let killed = child.kill();
        let waited = child.wait();
        match waited {
            Ok(_) => Ok(()),
            Err(error) => Err(format!(
                "E-WATCH: cannot reap process: {error} (kill: {killed:?})"
            )),
        }
    }

    fn cancel_compiler(&mut self) -> Result<(), String> {
        #[cfg(unix)]
        {
            self.poll()?;
            let Some(child) = self.0.as_ref() else {
                return Ok(());
            };
            // Import drivers supervise workers in separate groups. Give their
            // SIGTERM handler time to cancel and reap those workers before the
            // fallback kills this group. The leader remains our unreaped child.
            let group = -(child.id() as libc::pid_t);
            // SAFETY: this process group was created for our unreaped child;
            // its ID cannot have been reused since we still own that child.
            let signalled = unsafe { libc::kill(group, libc::SIGTERM) };
            if signalled != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(format!("E-WATCH: cannot cancel compiler group: {error}"));
                }
            }
            // Also reach a driver that moved itself to another process group.
            // SAFETY: this PID belongs to our unreaped child.
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if self.poll()?.is_some() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        self.stop()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(super) struct Supervisor {
    process: Process,
    #[cfg(unix)]
    _cancellation: Cancellation,
}

impl Supervisor {
    pub(super) fn new() -> Result<Self, String> {
        Ok(Self {
            process: Process(None),
            #[cfg(unix)]
            _cancellation: Cancellation::install()?,
        })
    }

    pub(super) fn spawn(&mut self, command: &mut Command) -> Result<(), String> {
        self.stop()?;
        self.process = Process::start(command)?;
        Ok(())
    }

    pub(super) fn poll(&mut self) -> Result<Option<ExitStatus>, String> {
        self.process.poll()
    }

    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.process.stop()
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        // Stop children before restoring the caller's signal dispositions.
        let _ = self.stop();
    }
}

/// Wait for a compiler in its own process group. A live Supervisor owns the
/// cancellation handlers. Cancellation kills and reaps the compiler group.
pub(super) fn status(command: &mut Command) -> Result<ExitStatus, String> {
    let mut process = Process::start(command)?;
    loop {
        if cancelled() {
            process.cancel_compiler()?;
            return Err("E-WATCH: cancelled".into());
        }
        if let Some(status) = process.poll()? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn owned_children_are_reaped_on_stop_and_compiler_cancellation() {
        let mut supervisor = Supervisor::new().unwrap();
        let mut sleeper = Command::new("/bin/sleep");
        sleeper.arg("30");
        supervisor.spawn(&mut sleeper).unwrap();
        let pid = supervisor.process.0.as_ref().unwrap().id();
        supervisor.stop().unwrap();
        let mut wait_status = 0;
        // SAFETY: status is writable and WNOHANG never blocks. There is no
        // child left to reap, regardless of later unrelated PID reuse.
        assert_eq!(
            unsafe { libc::waitpid(pid as libc::pid_t, &mut wait_status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert!(supervisor.poll().unwrap().is_none());

        // A driver must get time to clean its independently supervised import
        // worker. This shell only supplies a controllable test signal handler;
        // production command dispatch never evaluates shell source.
        let directory = std::env::temp_dir().join(format!(
            "rill-watch-signal-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let marker = directory.join("cleanup");
        let mut driver = Process::start(
            Command::new("/bin/sh")
                .stderr(std::process::Stdio::null())
                .arg("-c")
                .arg("trap 'printf cleaned > \"$1\"; exit 0' TERM; printf ready > \"$1.ready\"; while :; do sleep 30; done")
                .arg("driver")
                .arg(&marker),
        )
        .unwrap();
        let ready = directory.join("cleanup.ready");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !ready.exists() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists());
        driver.cancel_compiler().unwrap();
        assert_eq!(std::fs::read(&marker).unwrap(), b"cleaned");
        std::fs::remove_dir_all(directory).unwrap();

        let cancel_thread = thread::spawn(|| {
            thread::sleep(Duration::from_millis(50));
            CANCELLED.store(libc::SIGTERM, Ordering::Relaxed);
        });
        let result = status(Command::new("/bin/sleep").arg("30"));
        cancel_thread.join().unwrap();
        assert_eq!(result.unwrap_err(), "E-WATCH: cancelled");
        CANCELLED.store(0, Ordering::Relaxed);
        drop(supervisor);
        // Signal ownership is released for another watcher in this process.
        drop(Supervisor::new().unwrap());
    }
}
