//! Native images for the explicit Copy-value byte ABI.
use super::{Directory, NativeCompiler};
use crate::abi::{
    copy::{Function, Value},
    Export,
};
use crate::ast::Program;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::{fs, path::PathBuf, process::Command};
use std::{marker::PhantomData, rc::Rc};

/// A checked source-generated image. Editable ABI artifacts cannot be loaded.
pub struct CopyCandidate {
    _directory: Directory,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    library: PathBuf,
    manifest: String,
    functions: Vec<Function>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    identity: Vec<u8>,
    build_identity: String,
}

impl CopyCandidate {
    /// Compile closed pure Copy roots using the selected trusted host compiler.
    pub fn build(
        program: &Program,
        exports: &[Export],
        compiler: &NativeCompiler,
    ) -> Result<Self, String> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            build(program, exports, compiler)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (program, exports, compiler);
            Err(unsupported())
        }
    }
    pub fn functions(&self) -> &[Function] {
        &self.functions
    }
    pub fn manifest(&self) -> &str {
        &self.manifest
    }
    pub fn build_identity(&self) -> &str {
        &self.build_identity
    }
}

/// An exclusive synchronous native session with no escaping image pointers.
///
/// Native arithmetic/depth traps terminate the host. This API only returns
/// errors for admission, compilation, loading, compatibility and wire checks.
///
/// ```compile_fail
/// use rill::native_reload::CopySession;
/// fn send<T: Send>() {}
/// send::<CopySession>();
/// ```
/// ```compile_fail
/// use rill::native_reload::CopySession;
/// fn sync<T: Sync>() {}
/// sync::<CopySession>();
/// ```
pub struct CopySession {
    active: Active,
    generation: u64,
    _exclusive: PhantomData<Rc<()>>,
}

struct Active {
    // Native code must unload before its directory is removed.
    loaded: Loaded,
    candidate: CopyCandidate,
}
impl Active {
    fn load(candidate: CopyCandidate) -> Result<Self, String> {
        let loaded = Loaded::open(&candidate)?;
        Ok(Self { loaded, candidate })
    }
}
impl CopySession {
    pub fn new(candidate: CopyCandidate) -> Result<Self, String> {
        Ok(Self {
            active: Active::load(candidate)?,
            generation: 0,
            _exclusive: PhantomData,
        })
    }
    pub fn functions(&self) -> &[Function] {
        self.active.candidate.functions()
    }
    pub fn manifest(&self) -> &str {
        self.active.candidate.manifest()
    }
    pub fn build_identity(&self) -> &str {
        self.active.candidate.build_identity()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Replace only an exactly compatible complete contract. Stateful schema
    /// migrations use `live_reload::LiveSession` to stage new sessions instead.
    pub fn replace(&mut self, candidate: CopyCandidate) -> Result<u64, String> {
        if candidate.functions != self.active.candidate.functions {
            return Err("E-RELOAD: native Copy function contracts changed".into());
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

    pub fn call(&mut self, name: &str, arguments: &[Value]) -> Result<Value, String> {
        let slot = self
            .functions()
            .binary_search_by(|function| function.name.as_str().cmp(name))
            .map_err(|_| format!("E-RELOAD: unknown native Copy hook {name:?}"))?;
        let function = &self.active.candidate.functions[slot];
        let input = function.encode_arguments(arguments)?;
        let capacity = function.result.max_encoded_len()?;
        let output = self.active.loaded.call(slot, &input, capacity)?;
        function.decode_result(&output)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn build(
    program: &Program,
    exports: &[Export],
    compiler: &NativeCompiler,
) -> Result<CopyCandidate, String> {
    use super::{platform as base, Optimization};
    use std::os::unix::ffi::OsStrExt;
    let compiler_identity = base::CompilerIdentity::capture(&compiler.executable)?;
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
        "bridge": "rill-copy-v1",
        "rill_compiler": env!("RILL_COMPILER_ID"),
        "host_os": std::env::consts::OS,
        "host_arch": std::env::consts::ARCH,
        "pointer_bits": usize::BITS,
        "little_endian": cfg!(target_endian = "little"),
        "compiler_path": compiler_identity.path.as_os_str().as_bytes(),
        "compiler_sha256": compiler_identity.digest,
        "flags": flags,
    }))
    .map_err(|error| format!("E-RELOAD: cannot encode Copy build identity: {error}"))?;
    let mut exports = exports.to_vec();
    exports.sort_by(|a, b| a.c_name.cmp(&b.c_name));
    let artifact = crate::abi::copy::emit_copy_reload(program, &exports, &context)?;
    let directory = base::private_directory()?;
    let source = directory.0.join("copy.c");
    let library = directory.0.join(if cfg!(target_os = "macos") {
        "copy.dylib"
    } else {
        "copy.so"
    });
    fs::write(&source, artifact.c)
        .map_err(|error| format!("E-RELOAD: cannot write Copy candidate: {error}"))?;
    let output = Command::new(&compiler_identity.path)
        .args(flags)
        .arg(source)
        .arg("-o")
        .arg(&library)
        .current_dir(&directory.0)
        .output()
        .map_err(|error| format!("E-RELOAD: cannot execute native compiler: {error}"))?;
    if !output.status.success() {
        let detail: String = String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(8192)
            .collect();
        return Err(format!(
            "E-RELOAD: native Copy compiler failed with {}: {detail}",
            output.status
        ));
    }
    if base::CompilerIdentity::capture(&compiler.executable)? != compiler_identity {
        return Err("E-RELOAD: native compiler changed during Copy candidate build".into());
    }
    if !fs::symlink_metadata(&library).is_ok_and(|metadata| metadata.is_file()) {
        return Err("E-RELOAD: compiler did not produce a regular Copy candidate image".into());
    }
    let build_identity = crate::fingerprint::digest(&artifact.identity)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(CopyCandidate {
        _directory: directory,
        library,
        manifest: artifact.manifest,
        functions: artifact.exports,
        identity: artifact.identity,
        build_identity,
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
type Invoke = unsafe extern "C" fn(u32, *const u8, u32, *mut u8, u32, *mut u32) -> u32;
#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Loaded {
    handle: super::platform::Handle,
    invoke: Invoke,
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Loaded {
    fn open(candidate: &CopyCandidate) -> Result<Self, String> {
        use super::platform as base;
        let handle = base::open_identity(
            &candidate.library,
            &candidate.identity,
            c"rill_abi_copy_identity_v1",
        )?;
        let invoke = unsafe {
            std::mem::transmute::<*mut libc::c_void, Invoke>(base::symbol(
                handle.0,
                c"rill_abi_invoke_copy_v1",
            )?)
        };
        Ok(Self { handle, invoke })
    }
    fn call(&mut self, slot: usize, input: &[u8], capacity: usize) -> Result<Vec<u8>, String> {
        // Keep the handle live throughout the call; no native pointer escapes.
        let _keep_alive = &self.handle;
        let mut output = vec![0; capacity];
        let mut length = 0;
        let status = unsafe {
            (self.invoke)(
                slot as u32,
                input.as_ptr(),
                input.len() as u32,
                output.as_mut_ptr(),
                capacity as u32,
                &mut length,
            )
        };
        if status != 0 {
            return Err(format!(
                "E-RELOAD: native Copy dispatcher rejected call ({status})"
            ));
        }
        if length as usize > output.len() {
            return Err("E-RELOAD: native Copy dispatcher returned an oversized result".into());
        }
        output.truncate(length as usize);
        Ok(output)
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn unsupported() -> String {
    "E-RELOAD: native Copy reload requires macOS or Linux".into()
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
struct Loaded;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl Loaded {
    fn open(_: &CopyCandidate) -> Result<Self, String> {
        Err(unsupported())
    }
    fn call(&mut self, _: usize, _: &[u8], _: usize) -> Result<Vec<u8>, String> {
        Err(unsupported())
    }
}
