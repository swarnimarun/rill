//! Fixed synchronous native renderer dispatcher used by the example and tests.

use super::app::Renderer;
use std::ffi::{CStr, CString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

pub struct LoadedRenderer {
    handle: NonNull<libc::c_void>,
    dispatch: Dispatch,
    opened: bool,
    cleanup_directory: Option<TemporaryDirectory>,
}

/// Own temporary renderer files until compilation or loading finishes.
pub struct TemporaryDirectory(pub PathBuf);
impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

type Dispatch = unsafe extern "C" fn(u32, *const i32, u32) -> i32;

impl LoadedRenderer {
    pub fn open(
        path: &Path,
        cleanup_directory: Option<TemporaryDirectory>,
    ) -> Result<Self, String> {
        let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
        let handle =
            NonNull::new(unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) })
                .ok_or_else(loader_error)?;
        let symbol = unsafe { libc::dlsym(handle.as_ptr(), c"rill_renderer_v1".as_ptr()) };
        if symbol.is_null() {
            unsafe { libc::dlclose(handle.as_ptr()) };
            return Err("renderer has no rill_renderer_v1 dispatcher".into());
        }
        // The adapter header fixes this signature; arbitrary C APIs do not cross it.
        let dispatch = unsafe { std::mem::transmute::<*mut libc::c_void, Dispatch>(symbol) };
        let mut renderer = Self {
            handle,
            dispatch,
            opened: false,
            cleanup_directory,
        };
        if unsafe { (renderer.dispatch)(1, std::ptr::null(), 0) } != 0 {
            return Err("renderer could not open a real window/device".into());
        }
        renderer.opened = true;
        Ok(renderer)
    }
}

impl Renderer for LoadedRenderer {
    fn frame(&mut self, x: i32, color: i32) -> Result<bool, String> {
        match unsafe { (self.dispatch)(2, [x, color].as_ptr(), 2) } {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("renderer frame failed".into()),
        }
    }

    fn close(&mut self) -> Result<(), String> {
        if self.opened {
            self.opened = false;
            if unsafe { (self.dispatch)(3, std::ptr::null(), 0) } != 0 {
                return Err("renderer close failed".into());
            }
        }
        Ok(())
    }
}

impl Drop for LoadedRenderer {
    fn drop(&mut self) {
        let _ = self.close();
        unsafe { libc::dlclose(self.handle.as_ptr()) };
        drop(self.cleanup_directory.take());
    }
}

fn loader_error() -> String {
    let error = unsafe { libc::dlerror() };
    if error.is_null() {
        "cannot load renderer".into()
    } else {
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    }
}
