//! Native build preparation. Header tools run here, before ordinary resolution;
//! editor analysis only reads snapshots published after successful compilation.
use rill::{ast::Program, c_import, modules};
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

pub(super) struct Prepared {
    pub program: Program,
    pub cflags: Vec<OsString>,
    pub link_args: Vec<OsString>,
    snapshots: Vec<modules::headers::Snapshot>,
}

impl Prepared {
    pub fn publish(&mut self) -> Result<(), String> {
        for snapshot in self.snapshots.drain(..) {
            // A changed search path can make a valid build snapshot stale.
            // Keep the native result; editor analysis will require a refresh.
            snapshot.publish().map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

pub(super) fn load(
    entry: &Path,
    compiler: &OsStr,
    zig_cc: bool,
    arguments: Vec<OsString>,
    interpret: bool,
) -> Result<Prepared, String> {
    let graph = modules::load(entry).map_err(|error| error.to_string())?;
    let native = modules::headers::native_config(&graph).map_err(|error| error.to_string())?;
    if interpret
        && (!native.includes.is_empty() || !native.links.is_empty() || !native.packages.is_empty())
    {
        return Err(
            "--interpret cannot execute native dependencies; use the default native run".into(),
        );
    }
    let resolved = native.resolve()?;
    let mut cflags = resolved.cflags;
    cflags.extend(arguments);
    c_import::validate_compile_args(&cflags)?;
    let requests = modules::headers::requests(&graph).map_err(|error| error.to_string())?;
    let needs_import = requests.iter().any(|request| !request.names.is_empty());
    if needs_import {
        complete_sdk(&mut cflags)?;
    }
    let explicit_resource = cflags.iter().any(|argument| {
        argument.to_str().is_some_and(|argument| {
            argument == "-resource-dir" || argument.starts_with("-resource-dir=")
        })
    });
    let resource_dir = if needs_import && !explicit_resource {
        resource_directory(compiler, zig_cc)?
    } else {
        None
    };
    let mut expanded = graph.clone();
    let mut snapshots = Vec::new();
    for request in requests {
        if request.names.is_empty() {
            continue;
        }
        // The backend emits one C translation unit. Its complete ordered header
        // preamble and flags must also feed extraction and target validation.
        let selection = c_import::selected::Automatic {
            native: c_import::Config {
                includes: native.includes.clone(),
                ..c_import::Config::default()
            },
            names: request.names,
            c_args: cflags.clone(),
            resource_dir: resource_dir.clone(),
        };
        let imported = generate(&selection, compiler, zig_cc)?;
        let snapshot = modules::headers::prepare_with_state(
            &graph,
            &request.module,
            &imported.module,
            &imported.inputs,
        )
        .map_err(|error| error.to_string())?;
        modules::headers::apply(&mut expanded, &request.module, &imported.module)
            .map_err(|error| error.to_string())?;
        snapshots.push(snapshot);
    }
    let program = modules::compile::lower_graph(&expanded).map_err(|error| error.to_string())?;
    Ok(Prepared {
        program,
        cflags,
        link_args: resolved.link_args,
        snapshots,
    })
}

fn generate(
    selection: &c_import::selected::Automatic,
    compiler: &OsStr,
    zig_cc: bool,
) -> Result<c_import::selected::Generated, String> {
    #[cfg(all(feature = "c-import", unix))]
    {
        super::import_worker::generate_automatic(
            selection,
            compiler,
            zig_cc,
            std::time::Duration::from_secs(30),
        )
    }
    #[cfg(not(all(feature = "c-import", unix)))]
    {
        let _ = (selection, compiler, zig_cc);
        Err("E-C-IMPORT: automatic @include requires the c-import feature and a Unix host; explicit extern declarations remain available".into())
    }
}

fn resource_directory(
    compiler: &OsStr,
    zig_cc: bool,
) -> Result<Option<std::path::PathBuf>, String> {
    let mut command = std::process::Command::new(compiler);
    if zig_cc {
        command.arg("cc");
    }
    let Some(output) = discovery_output(command.arg("-print-resource-dir"))? else {
        return Ok(None);
    };
    let Some(path) = std::str::from_utf8(&output)
        .ok()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return Ok(None);
    };
    let path = std::path::PathBuf::from(path);
    Ok(path.join("include/stdint.h").is_file().then_some(path))
}

fn discovery_output(command: &mut std::process::Command) -> Result<Option<Vec<u8>>, String> {
    #[cfg(all(feature = "c-import", unix))]
    {
        super::import_worker::probe(command, std::time::Duration::from_secs(2))
    }
    #[cfg(not(all(feature = "c-import", unix)))]
    {
        let _ = command;
        Ok(None)
    }
}

// Apple's driver normally discovers the host SDK itself. libclang needs the
// same explicit search root. Never substitute the host SDK for a chosen target
// or sysroot. SDK discovery is build preparation, never an editor operation.
#[cfg(all(target_os = "macos", feature = "c-import", unix))]
fn complete_sdk(flags: &mut Vec<OsString>) -> Result<(), String> {
    if flags.iter().any(|flag| {
        flag.to_str().is_some_and(|flag| {
            matches!(flag, "-isysroot" | "--sysroot" | "-target" | "--target")
                || flag.starts_with("--sysroot=")
                || flag.starts_with("--target=")
        })
    }) {
        return Ok(());
    }
    let sdk = if let Some(path) = std::env::var_os("SDKROOT").filter(|path| !path.is_empty()) {
        path
    } else {
        let output = discovery_output(std::process::Command::new("xcrun").arg("--show-sdk-path"))?;
        let Some(output) = output else {
            return Err("E-C-IMPORT: cannot discover the macOS SDK; select an explicit --c-arg -isysroot and SDK path".into());
        };
        OsString::from(
            std::str::from_utf8(&output)
                .map_err(|_| "E-C-IMPORT: SDK path is not UTF-8; set SDKROOT explicitly")?
                .trim(),
        )
    };
    if sdk.is_empty() {
        return Err("E-C-IMPORT: empty macOS SDK path".into());
    }
    flags.push("-isysroot".into());
    flags.push(sdk);
    Ok(())
}

#[cfg(not(all(target_os = "macos", feature = "c-import", unix)))]
fn complete_sdk(_: &mut Vec<OsString>) -> Result<(), String> {
    Ok(())
}
