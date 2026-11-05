//! Synchronous native replacement for checked, closed `i32` library functions.
//!
//! Compile a [`NativeCandidate`] from Rill source, then install it between calls
//! to a [`NativeSession`]. The host owns state and passes it as integer arguments
//! and results. An unsuccessful build or replacement preserves the active code.
//! No function pointer or borrowed native allocation escapes this API.
//!
//! This experimental boundary supports native macOS and Linux C11 compilers.
//! The selected compiler and its environment must be trusted, just as when
//! building and running an executable. Candidates use private, unique temporary
//! directories; there is deliberately no API to load an arbitrary library path.
//! Generated arithmetic and call-depth traps terminate the host process. This
//! API does not recover traps, migrate owned state or patch active stack frames.

use std::ffi::OsString;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;

use crate::abi::{Export, ReloadExport};
use crate::ast::Program;

/// Fixed native compiler profiles supported by the reload bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Optimization {
    O0,
    O3,
}

/// A trusted, native-host C11 compiler executable and its optimization profile.
///
/// Arguments are passed directly to this executable, without shell expansion.
/// Compiler wrappers may be selected as executables; arbitrary extra compiler
/// or linker flags and cross compilation are outside this boundary.
#[derive(Clone, Debug)]
pub struct NativeCompiler {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    executable: OsString,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    optimization: Optimization,
}

impl NativeCompiler {
    pub fn new(executable: impl Into<OsString>, optimization: Optimization) -> Self {
        let executable = executable.into();
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let _ = (executable, optimization);
        Self {
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            executable,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            optimization,
        }
    }
}

impl Default for NativeCompiler {
    fn default() -> Self {
        Self::new("cc", Optimization::O0)
    }
}

/// A privately compiled candidate. Building it does not alter an active session.
pub struct NativeCandidate {
    // Keep the directory alive until its library has been unloaded.
    _directory: Directory,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    library: PathBuf,
    manifest: String,
    exports: Vec<ReloadExport>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    identity: Vec<u8>,
    build_identity: String,
}

impl NativeCandidate {
    /// Check every source declaration and compile explicit closed `i32` roots.
    ///
    /// The signature and rejection rules match [`crate::abi::emit_i32_library`].
    /// Public names are canonicalized so declaration and export ordering do not
    /// affect replacement compatibility. Only this source-generated candidate
    /// can be installed; editable public ABI artifacts cannot be loaded.
    pub fn build(
        program: &Program,
        exports: &[Export],
        compiler: &NativeCompiler,
    ) -> Result<Self, String> {
        platform::build(program, exports, compiler)
    }

    pub fn manifest(&self) -> &str {
        &self.manifest
    }

    /// SHA-256 of the exact versioned bridge identity expected at load time.
    pub fn build_identity(&self) -> &str {
        &self.build_identity
    }
}

/// One active native generation with an exclusive boundary between calls.
///
/// Calls and replacement require `&mut self`. The session is neither `Send` nor
/// `Sync`; no native frame can remain active when its library is closed. This
/// first boundary has no callbacks, foreign values, owners or escaping native
/// pointers. A host can keep integer state across calls and replacements.
///
/// ```compile_fail
/// use rill::native_reload::NativeSession;
/// fn requires_send<T: Send>() {}
/// requires_send::<NativeSession>();
/// ```
///
/// ```compile_fail
/// use rill::native_reload::NativeSession;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<NativeSession>();
/// ```
pub struct NativeSession {
    active: Active,
    generation: i32,
    _synchronous: PhantomData<Rc<()>>,
}

struct Active {
    // Field drop order matters: close the handle before removing its files.
    loaded: platform::Loaded,
    candidate: NativeCandidate,
}

impl Active {
    fn load(candidate: NativeCandidate) -> Result<Self, String> {
        let loaded = platform::Loaded::open(&candidate)?;
        Ok(Self { loaded, candidate })
    }
}

impl NativeSession {
    /// Load and validate the candidate's version, signatures and build identity.
    pub fn new(candidate: NativeCandidate) -> Result<Self, String> {
        Ok(Self {
            active: Active::load(candidate)?,
            generation: 0,
            _synchronous: PhantomData,
        })
    }

    /// Install a candidate only if its complete public contract is unchanged.
    ///
    /// Validation and loading finish before publication. On any error the old
    /// generation remains callable. The old handle closes only after previous
    /// synchronous calls have returned. A successful call returns the new
    /// generation, beginning with 1 for the first replacement.
    pub fn replace(&mut self, candidate: NativeCandidate) -> Result<i32, String> {
        if candidate.exports != self.active.candidate.exports {
            return Err("E-RELOAD: native export names or integer signatures changed".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("E-RELOAD: native generation counter exhausted")?;
        let active = Active::load(candidate)?;
        self.active = active;
        self.generation = generation;
        Ok(generation)
    }

    /// Call an explicitly registered C name with exactly its checked arguments.
    ///
    /// Lookup and arity errors return `Err`. Generated runtime traps terminate
    /// the process with status 1, as in the ordinary scalar C ABI.
    pub fn call_i32(&mut self, c_name: &str, arguments: &[i32]) -> Result<i32, String> {
        let slot = self
            .active
            .candidate
            .exports
            .binary_search_by(|export| export.name.as_str().cmp(c_name))
            .map_err(|_| format!("E-RELOAD: unknown native hook {c_name:?}"))?;
        let export = &self.active.candidate.exports[slot];
        if arguments.len() != export.arity {
            return Err(format!(
                "E-RELOAD: {c_name} expects {} arguments, got {}",
                export.arity,
                arguments.len()
            ));
        }
        self.active.loaded.call(slot, arguments)
    }

    pub fn generation(&self) -> i32 {
        self.generation
    }

    pub fn build_identity(&self) -> &str {
        self.active.candidate.build_identity()
    }
}

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        // Cleanup failure cannot turn a published replacement into an error.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod platform {
    use super::*;
    use std::ffi::{CStr, CString, OsStr};
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::Path;
    use std::process::Command;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // glibc before 2.34 keeps the loader entry points in libdl.
    #[cfg(target_os = "linux")]
    #[link(name = "dl")]
    unsafe extern "C" {}

    type Identity = unsafe extern "C" fn(*mut u8, u32) -> u32;
    type Invoke = unsafe extern "C" fn(u32, *const i32, u32, *mut i32) -> u32;

    pub(super) struct Loaded {
        handle: NonNull<libc::c_void>,
        invoke: Invoke,
    }

    impl Loaded {
        pub(super) fn open(candidate: &NativeCandidate) -> Result<Self, String> {
            let path = CString::new(candidate.library.as_os_str().as_bytes())
                .map_err(|_| "E-RELOAD: native library path contains NUL")?;
            // The only loadable paths belong to source-generated candidates in
            // private directories. The selected compiler is trusted executable
            // authority, not an unverified artifact supplied by another party.
            let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            let handle =
                NonNull::new(handle).ok_or_else(|| loader_error("cannot load candidate"))?;
            // Own the handle immediately so every following error closes it.
            let guard = Handle(handle);
            let identity = unsafe {
                std::mem::transmute::<*mut libc::c_void, Identity>(symbol(
                    handle,
                    c"rill_abi_identity_v1",
                )?)
            };
            let expected = &candidate.identity;
            // A bounded copy avoids borrowing pointers or trusting a returned
            // native allocation. Check the versioned identity before resolving
            // or calling the dispatcher.
            let length = unsafe { identity(std::ptr::null_mut(), 0) };
            if length as usize != expected.len() {
                return Err("E-RELOAD: native ABI or build identity length mismatch".into());
            }
            let mut actual = vec![0; expected.len()];
            let copied = unsafe { identity(actual.as_mut_ptr(), length) };
            if copied != length || actual != *expected {
                return Err("E-RELOAD: native ABI or build identity mismatch".into());
            }
            let invoke = unsafe {
                std::mem::transmute::<*mut libc::c_void, Invoke>(symbol(
                    handle,
                    c"rill_abi_invoke_v1",
                )?)
            };
            std::mem::forget(guard);
            Ok(Self { handle, invoke })
        }

        pub(super) fn call(&mut self, slot: usize, arguments: &[i32]) -> Result<i32, String> {
            let mut result = 0;
            // The checked contract bounds slots to 64 and arity to 32. The
            // bridge has one fixed C signature and directly calls verified
            // private function IDs; no guessed per-endpoint casts are used.
            let status = unsafe {
                (self.invoke)(
                    slot as u32,
                    arguments.as_ptr(),
                    arguments.len() as u32,
                    &mut result,
                )
            };
            if status != 0 {
                return Err(format!(
                    "E-RELOAD: native dispatcher rejected call ({status})"
                ));
            }
            Ok(result)
        }
    }

    impl Drop for Loaded {
        fn drop(&mut self) {
            // No callable or borrowed pointer escapes Loaded, and its session
            // cannot be dropped while an exclusive synchronous call is active.
            unsafe { libc::dlclose(self.handle.as_ptr()) };
        }
    }

    struct Handle(NonNull<libc::c_void>);

    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { libc::dlclose(self.0.as_ptr()) };
        }
    }

    fn symbol(handle: NonNull<libc::c_void>, name: &CStr) -> Result<*mut libc::c_void, String> {
        unsafe { libc::dlerror() };
        let pointer = unsafe { libc::dlsym(handle.as_ptr(), name.as_ptr()) };
        let error = unsafe { libc::dlerror() };
        if !error.is_null() {
            let detail = unsafe { CStr::from_ptr(error) }.to_string_lossy();
            return Err(format!("E-RELOAD: missing versioned bridge: {detail}"));
        }
        if pointer.is_null() {
            return Err("E-RELOAD: null versioned bridge symbol".into());
        }
        Ok(pointer)
    }

    fn loader_error(context: &str) -> String {
        let error = unsafe { libc::dlerror() };
        if error.is_null() {
            return format!("E-RELOAD: {context}");
        }
        let detail = unsafe { CStr::from_ptr(error) }.to_string_lossy();
        format!("E-RELOAD: {context}: {detail}")
    }

    #[derive(PartialEq, Eq)]
    struct CompilerIdentity {
        path: PathBuf,
        digest: [u8; 32],
    }

    impl CompilerIdentity {
        fn capture(executable: &OsStr) -> Result<Self, String> {
            let path = if executable.as_bytes().contains(&b'/') {
                PathBuf::from(executable)
            } else {
                let search = std::env::var_os("PATH")
                    .ok_or("E-RELOAD: PATH is unset; select a compiler executable")?;
                std::env::split_paths(&search)
                    .map(|directory| directory.join(executable))
                    .find(|path| executable_file(path))
                    .ok_or_else(|| format!("E-RELOAD: cannot find compiler {executable:?}"))?
            };
            let path = fs::canonicalize(path)
                .map_err(|error| format!("E-RELOAD: cannot resolve compiler: {error}"))?;
            if !executable_file(&path) {
                return Err("E-RELOAD: selected compiler is not an executable file".into());
            }
            let bytes = fs::read(&path)
                .map_err(|error| format!("E-RELOAD: cannot fingerprint compiler: {error}"))?;
            Ok(Self {
                path,
                digest: crate::fingerprint::digest(&bytes),
            })
        }
    }

    fn executable_file(path: &Path) -> bool {
        fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }

    pub(super) fn build(
        program: &Program,
        exports: &[Export],
        compiler: &NativeCompiler,
    ) -> Result<NativeCandidate, String> {
        let identity = CompilerIdentity::capture(&compiler.executable)?;
        let optimization = match compiler.optimization {
            Optimization::O0 => "-O0",
            Optimization::O3 => "-O3",
        };
        let shared = if cfg!(target_os = "macos") {
            "-dynamiclib"
        } else {
            "-shared"
        };
        let flags = ["-std=c11", optimization, "-fPIC", shared];
        let context = serde_json::to_vec(&serde_json::json!({
            "bridge": 1,
            "rill_compiler": env!("RILL_COMPILER_ID"),
            "host_os": std::env::consts::OS,
            "host_arch": std::env::consts::ARCH,
            "pointer_bits": usize::BITS,
            "little_endian": cfg!(target_endian = "little"),
            "compiler_path": identity.path.as_os_str().as_bytes(),
            "compiler_sha256": identity.digest,
            "flags": flags,
        }))
        .map_err(|error| format!("E-RELOAD: cannot encode native build identity: {error}"))?;
        let artifact = crate::abi::emit_i32_reload(program, exports, &context)?;
        let directory = private_directory()?;
        let source = directory.0.join("library.c");
        let library = directory.0.join(if cfg!(target_os = "macos") {
            "library.dylib"
        } else {
            "library.so"
        });
        fs::write(&source, artifact.c)
            .map_err(|error| format!("E-RELOAD: cannot write candidate C: {error}"))?;
        let output = Command::new(&identity.path)
            .args(flags)
            .arg(&source)
            .arg("-o")
            .arg(&library)
            .current_dir(&directory.0)
            .output()
            .map_err(|error| format!("E-RELOAD: cannot execute native compiler: {error}"))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail: String = detail.chars().take(8192).collect();
            return Err(format!(
                "E-RELOAD: native compiler failed with {}: {detail}",
                output.status
            ));
        }
        if CompilerIdentity::capture(&compiler.executable)? != identity {
            return Err("E-RELOAD: native compiler changed during candidate build".into());
        }
        if !fs::symlink_metadata(&library).is_ok_and(|metadata| metadata.is_file()) {
            return Err("E-RELOAD: compiler did not produce a regular candidate library".into());
        }
        let build_identity = crate::fingerprint::digest(&artifact.identity)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(NativeCandidate {
            _directory: directory,
            library,
            manifest: artifact.manifest,
            exports: artifact.exports,
            identity: artifact.identity,
            build_identity,
        })
    }

    fn private_directory() -> Result<Directory, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = fs::canonicalize(std::env::temp_dir())
            .map_err(|error| format!("E-RELOAD: cannot resolve temporary directory: {error}"))?;
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("E-RELOAD: system clock precedes epoch: {error}"))?
            .as_nanos();
        for _ in 0..128 {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                "rill-native-reload-{}-{epoch}-{sequence}",
                std::process::id()
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Directory(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "E-RELOAD: cannot create private directory: {error}"
                    ));
                }
            }
        }
        Err("E-RELOAD: cannot allocate unique private candidate directory".into())
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use std::fmt::Write;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn candidate(increment: i32) -> NativeCandidate {
        let program =
            crate::parser::parse(&format!("fn update(x: i32) -> i32 {{ x + {increment} }}"))
                .unwrap();
        NativeCandidate::build(
            &program,
            &[Export {
                source: "update".into(),
                c_name: "rill_update".into(),
            }],
            &NativeCompiler::default(),
        )
        .unwrap()
    }

    fn rewrite_library(candidate: &NativeCandidate, source: &str) {
        let c = candidate._directory.0.join("fault.c");
        fs::write(&c, source).unwrap();
        let status = Command::new("cc")
            .args([
                "-std=c11",
                "-fPIC",
                if cfg!(target_os = "macos") {
                    "-dynamiclib"
                } else {
                    "-shared"
                },
            ])
            .arg(c)
            .arg("-o")
            .arg(&candidate.library)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn assert_retained(session: &mut NativeSession) {
        assert_eq!(session.generation(), 0);
        assert_eq!(session.call_i32("rill_update", &[10]).unwrap(), 11);
    }

    #[test]
    fn invalid_library_and_missing_bridge_do_not_publish() {
        let mut session = NativeSession::new(candidate(1)).unwrap();
        let invalid = candidate(9);
        fs::write(&invalid.library, b"not a shared library").unwrap();
        let error = session.replace(invalid).unwrap_err();
        assert!(error.contains("cannot load candidate"), "{error}");
        assert_retained(&mut session);

        let missing = candidate(9);
        rewrite_library(&missing, "int unrelated(void) { return 0; }\n");
        let error = session.replace(missing).unwrap_err();
        assert!(error.contains("missing versioned bridge"), "{error}");
        assert_retained(&mut session);

        let unbounded = candidate(9);
        rewrite_library(
            &unbounded,
            "#include <stdint.h>\nuint32_t rill_abi_identity_v1(unsigned char *out, uint32_t cap) { (void)out; (void)cap; return UINT32_MAX; }\n",
        );
        let error = session.replace(unbounded).unwrap_err();
        assert!(error.contains("identity length mismatch"), "{error}");
        assert_retained(&mut session);
    }

    #[test]
    fn stale_build_identity_and_missing_dispatcher_do_not_publish() {
        let mut session = NativeSession::new(candidate(1)).unwrap();
        let stale = candidate(9);
        fs::copy(&session.active.candidate.library, &stale.library).unwrap();
        let error = session.replace(stale).unwrap_err();
        assert!(error.contains("build identity"), "{error}");
        assert_retained(&mut session);

        let missing = candidate(9);
        let mut c = String::from("#include <stdint.h>\nstatic const unsigned char identity[] = {");
        for byte in &missing.identity {
            write!(c, "{byte},").unwrap();
        }
        c.push_str("};\nuint32_t rill_abi_identity_v1(unsigned char *out, uint32_t cap) {\nuint32_t n = sizeof(identity);\nif (out && cap >= n) for (uint32_t i = 0; i < n; ++i) out[i] = identity[i];\nreturn n;\n}\n");
        rewrite_library(&missing, &c);
        let error = session.replace(missing).unwrap_err();
        assert!(error.contains("missing versioned bridge"), "{error}");
        assert_retained(&mut session);
    }

    #[test]
    fn each_generation_owns_a_unique_private_directory_until_quiescent_drop() {
        let first = candidate(1);
        assert!(first.library.is_absolute());
        assert_eq!(
            fs::metadata(&first._directory.0)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let mut previous = first._directory.0.clone();
        let mut session = NativeSession::new(first).unwrap();
        for generation in 1..=3 {
            let next = candidate(1);
            let directory = next._directory.0.clone();
            assert_ne!(directory, previous);
            assert!(previous.exists());
            assert_eq!(session.replace(next).unwrap(), generation);
            assert!(!previous.exists());
            assert!(directory.exists());
            assert_eq!(session.call_i32("rill_update", &[10]).unwrap(), 11);
            previous = directory;
        }
        drop(session);
        assert!(!previous.exists());
    }

    #[test]
    fn generation_exhaustion_preserves_the_old_handle_and_drops_the_candidate() {
        let mut session = NativeSession::new(candidate(1)).unwrap();
        session.generation = i32::MAX;
        let next = candidate(9);
        let directory = next._directory.0.clone();
        assert!(session.replace(next).unwrap_err().contains("exhausted"));
        assert!(!directory.exists());
        assert_eq!(session.generation(), i32::MAX);
        assert_eq!(session.call_i32("rill_update", &[10]).unwrap(), 11);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;

    pub(super) fn build(
        _: &Program,
        _: &[Export],
        _: &NativeCompiler,
    ) -> Result<NativeCandidate, String> {
        Err("E-RELOAD: native reload currently requires macOS or Linux".into())
    }

    pub(super) struct Loaded;

    impl Loaded {
        pub(super) fn open(_: &NativeCandidate) -> Result<Self, String> {
            Err("E-RELOAD: native reload currently requires macOS or Linux".into())
        }

        pub(super) fn call(&mut self, _: usize, _: &[i32]) -> Result<i32, String> {
            Err("E-RELOAD: native reload currently requires macOS or Linux".into())
        }
    }
}
