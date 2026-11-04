#![cfg(feature = "c-import")]

use rill::c_import::{
    selected::{generate_automatic, Automatic},
    Config,
};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill automatic import {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("api.h"),
            include_str!("fixtures/c_import_selected/api.h"),
        )
        .unwrap();
        fs::write(
            path.join("unsupported.h"),
            include_str!("fixtures/c_import_selected/unsupported.h"),
        )
        .unwrap();
        fs::write(
            path.join("entry.h"),
            "#include <api.h>\n#include <unsupported.h>\n",
        )
        .unwrap();
        Self(path)
    }

    fn request(&self, names: &[&str]) -> Automatic {
        Automatic {
            native: Config {
                includes: vec!["entry.h".into()],
                ..Config::default()
            },
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            c_args: vec!["-I".into(), self.0.as_os_str().to_owned()],
            resource_dir: resource_dir().map(Path::to_path_buf),
        }
    }

    #[cfg(unix)]
    fn mutating_compiler(&self, after: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.0.join("mutating cc");
        let script = format!("#!/bin/sh\nprevious=''\nfor argument in \"$@\"; do\n if test \"$previous\" = '-I'; then directory=\"$argument\"; break; fi\n previous=\"$argument\"\ndone\ncc \"$@\"\nresult=$?\nif test \"$result\" -eq 0; then\n{after}\nfi\nexit \"$result\"\n");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn resource_dir() -> Option<&'static Path> {
    static RESOURCE: OnceLock<Option<PathBuf>> = OnceLock::new();
    RESOURCE
        .get_or_init(|| {
            if let Some(directory) = std::env::var_os("RILL_TEST_CLANG_RESOURCE_DIR") {
                return Some(directory.into());
            }
            let compiler =
                std::env::var_os("CLANG_PATH").unwrap_or_else(|| OsString::from("clang"));
            let output = Command::new(compiler)
                .arg("-print-resource-dir")
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
            path.join("include/stdint.h").is_file().then_some(path)
        })
        .as_deref()
}

#[test]
fn automatic_names_import_transitive_functions_constants_and_real_dependencies() {
    let fixture = Fixture::new();
    let generated = generate_automatic(
        &fixture.request(&["selected_add", "SELECTED_ENUM"]),
        OsStr::new("cc"),
        false,
    )
    .unwrap();
    assert!(generated
        .module
        .contains("export extern fn selected_add(arg0: i32, arg1: i32) -> i32;"));
    assert!(generated
        .module
        .contains("export extern const SELECTED_ENUM: i32;"));
    assert!(!generated.module.contains("selected_variadic"));
    for header in ["entry.h", "api.h", "unsupported.h"] {
        assert!(
            generated
                .dependencies
                .iter()
                .any(|path| path.file_name() == Some(OsStr::new(header))),
            "{header}: {:?}",
            generated.dependencies
        );
    }
    assert!(generated
        .dependencies
        .iter()
        .all(|path| path.is_absolute() && path.is_file()));
}

#[test]
fn automatic_unknown_names_are_left_for_rill_resolution() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("entry.h"),
        "#include <api.h>\n#define formerly_missing(x) (x)\n#undef formerly_missing\n",
    )
    .unwrap();
    for names in [&["missing_rill_name", "formerly_missing"][..], &[][..]] {
        let generated =
            generate_automatic(&fixture.request(names), OsStr::new("cc"), false).unwrap();
        assert!(!generated.module.contains("export extern"));
        assert!(!generated.dependencies.is_empty());
    }
}

#[test]
fn automatic_unsupported_used_declarations_report_header_and_symbol() {
    let fixture = Fixture::new();
    for name in [
        "selected_variadic",
        "selected_callback",
        "SELECTED_FUNCTION_MACRO",
        "SELECTED_RUNTIME_VARIABLE",
        "SELECTED_STRING_CONSTANT",
    ] {
        let error =
            generate_automatic(&fixture.request(&[name]), OsStr::new("cc"), false).unwrap_err();
        assert!(
            error.contains("E-C-IMPORT:") && error.contains(name),
            "{error}"
        );
        assert!(error.contains("unsupported.h:"), "{error}");
    }
}

#[test]
fn automatic_import_shares_optimized_debug_and_preprocessing_flags() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("optimized.h"),
        "#include <stdint.h>\n#ifdef __OPTIMIZE__\nuint32_t automatic_optimized(uint32_t);\n#else\nint32_t automatic_optimized(int32_t);\n#endif\n",
    ).unwrap();
    let mut request = fixture.request(&[
        "selected_conditional",
        "SELECTED_CONDITIONAL",
        "automatic_optimized",
    ]);
    request.native.includes.push("optimized.h".into());
    request
        .c_args
        .extend(["-O2".into(), "-g2".into(), "-DSELECTED_ALTERNATE".into()]);
    let generated = generate_automatic(&request, OsStr::new("cc"), false).unwrap();
    assert!(generated
        .module
        .contains("export extern fn selected_conditional(arg0: u32) -> u32;"));
    assert!(generated
        .module
        .contains("export extern const SELECTED_CONDITIONAL: u32;"));
    assert!(generated
        .module
        .contains("export extern fn automatic_optimized(arg0: u32) -> u32;"));
}

#[test]
fn automatic_import_requires_the_selected_target_compiler() {
    let fixture = Fixture::new();
    let error = generate_automatic(
        &fixture.request(&["selected_add"]),
        fixture.0.join("missing cc").as_os_str(),
        false,
    )
    .unwrap_err();
    assert!(error.contains("cannot start"), "{error}");
}

#[test]
fn automatic_object_macros_keep_the_requested_function_spelling() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("aliases.h"), "#include <entry.h>\n#define HEADER_ADD selected_add\n#define HEADER_INDIRECT HEADER_ADD\nint32_t header_changed(int32_t);\n#define header_changed selected_add\n").unwrap();
    let mut request = fixture.request(&["HEADER_ADD", "HEADER_INDIRECT", "header_changed"]);
    request.native.includes = vec!["aliases.h".into()];
    let generated = generate_automatic(&request, OsStr::new("cc"), false).unwrap();
    for name in &request.names {
        assert!(
            generated.module.contains(&format!(
                "export extern fn {name}(arg0: i32, arg1: i32) -> i32;"
            )),
            "{}",
            generated.module
        );
    }
    let mut command_line = fixture.request(&["selected_add"]);
    command_line
        .c_args
        .push("-Dselected_add=renamed_add".into());
    let generated = generate_automatic(&command_line, OsStr::new("cc"), false).unwrap();
    assert!(generated
        .module
        .contains("export extern fn selected_add(arg0: i32, arg1: i32) -> i32;"));
}

#[test]
fn automatic_import_accepts_shared_warning_flags_without_weakening_abi_checks() {
    let fixture = Fixture::new();
    let mut request = fixture.request(&["selected_add", "SELECTED_ENUM"]);
    request.native.includes = vec!["api.h".into()];
    request.c_args.extend([
        "-Wall".into(),
        "-Wextra".into(),
        "-Wno-unused-function".into(),
    ]);
    let generated = generate_automatic(&request, OsStr::new("cc"), false).unwrap();
    assert!(generated
        .module
        .contains("export extern fn selected_add(arg0: i32, arg1: i32) -> i32;"));
    assert!(generated
        .module
        .contains("export extern const SELECTED_ENUM: i32;"));
    fs::write(
        fixture.0.join("alias.h"),
        "#include <api.h>\n#define public_add selected_add\n",
    )
    .unwrap();
    let mut request = fixture.request(&["public_add"]);
    request.native.includes = vec!["alias.h".into()];
    request
        .c_args
        .extend(["-Wpedantic".into(), "-Werror".into()]);
    let generated = generate_automatic(&request, OsStr::new("cc"), false).unwrap();
    assert!(generated
        .module
        .contains("export extern fn public_add(arg0: i32, arg1: i32) -> i32;"));
    for flag in ["-w", "-Wno-error"] {
        let mut request = fixture.request(&["selected_variadic"]);
        request.c_args.push(flag.into());
        assert!(generate_automatic(&request, OsStr::new("cc"), false)
            .unwrap_err()
            .contains("variadic"));
    }
}

#[test]
fn automatic_macro_aliases_preserve_abi_rejections_and_real_preprocessing() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("aliases.h"), "#include <entry.h>\n#define HEADER_STATIC selected_internal\n#define HEADER_VARIADIC selected_variadic\n#define HEADER_FNLIKE(x) selected_add(x, 1)\nint32_t HEADER_EXISTING(int32_t);\n#define HEADER_EXISTING(x) selected_add(x, 1)\n#define formerly_function(x) selected_add(x, 1)\n#undef formerly_function\nint32_t formerly_function(int32_t);\n#define formerly_object selected_add\n#undef formerly_object\nint32_t formerly_object(int32_t);\n#define formerly_function_constant(x) (x)\n#undef formerly_function_constant\n#define formerly_function_constant 42\n").unwrap();
    for name in [
        "HEADER_STATIC",
        "HEADER_VARIADIC",
        "HEADER_FNLIKE",
        "HEADER_EXISTING",
    ] {
        let mut request = fixture.request(&[name]);
        request.native.includes = vec!["aliases.h".into()];
        let error = generate_automatic(&request, OsStr::new("cc"), false).unwrap_err();
        assert!(error.contains(name) && error.contains(".h:"), "{error}");
    }
    let mut request = fixture.request(&[
        "formerly_function",
        "formerly_object",
        "formerly_function_constant",
    ]);
    request.native.includes = vec!["aliases.h".into()];
    let generated = generate_automatic(&request, OsStr::new("cc"), false).unwrap();
    for name in ["formerly_function", "formerly_object"] {
        assert!(
            generated
                .module
                .contains(&format!("export extern fn {name}(arg0: i32) -> i32;")),
            "{}",
            generated.module
        );
    }
    assert!(generated
        .module
        .contains("export extern const formerly_function_constant: i32;"));
}

#[test]
#[cfg(unix)]
fn automatic_import_rejects_header_changes_after_exact_target_validation() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("race.h"),
        "#include <stdint.h>\n#define RACE UINT32_C(42)\n",
    )
    .unwrap();
    let compiler = fixture.mutating_compiler(
        "printf '%s\\n' '#include <stdint.h>' '#define RACE INT32_C(42)' > \"$directory/race.h\"",
    );
    let mut request = fixture.request(&["RACE"]);
    request.native.includes = vec!["race.h".into()];
    let error = generate_automatic(&request, compiler.as_os_str(), false).unwrap_err();
    assert!(error.contains("changed during automatic import"), "{error}");
    assert!(fs::read_to_string(fixture.0.join("race.h"))
        .unwrap()
        .contains("#define RACE INT32_C(42)"));
}

#[test]
#[cfg(unix)]
fn automatic_import_rejects_new_optional_headers_after_target_validation() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("race.h"), "#include <stdint.h>\n#if __has_include(\"optional.h\")\n#define RACE UINT32_C(42)\n#else\n#define RACE INT32_C(42)\n#endif\n").unwrap();
    let compiler = fixture.mutating_compiler(
        "printf '%s\\n' '/* newly available header */' > \"$directory/optional.h\"",
    );
    let mut request = fixture.request(&["RACE"]);
    request.native.includes = vec!["race.h".into()];
    let error = generate_automatic(&request, compiler.as_os_str(), false).unwrap_err();
    assert!(error.contains("changed during automatic import"), "{error}");
    assert!(fixture.0.join("optional.h").is_file());
}
