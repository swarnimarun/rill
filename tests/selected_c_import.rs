#![cfg(all(feature = "c-import", unix))]

use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "rill selected import {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        for (name, contents) in [
            ("api.h", include_str!("fixtures/c_import_selected/api.h")),
            ("api.c", include_str!("fixtures/c_import_selected/api.c")),
            (
                "unsupported.h",
                include_str!("fixtures/c_import_selected/unsupported.h"),
            ),
        ] {
            fs::write(directory.join(name), contents).unwrap();
        }
        Self(directory)
    }

    fn command(&self, header: &str) -> Command {
        self.command_with_compiler(header, OsStr::new("cc"))
    }

    fn command_with_compiler(&self, header: &str, compiler: &OsStr) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .args(["import-c", "--header", header, "--c-arg", "-I", "--c-arg"])
            .arg(&self.0)
            .arg("--cc")
            .arg(compiler);
        if let Some(directory) = resource_dir() {
            command.arg("--clang-resource-dir").arg(directory);
        }
        command
    }

    fn select(&self, functions: &[&str], constants: &[&str]) -> Command {
        let mut command = self.command("api.h");
        for name in functions {
            command.args(["--function", name]);
        }
        for name in constants {
            command.args(["--constant", name]);
        }
        command
    }

    fn import(&self, functions: &[&str], constants: &[&str]) -> String {
        let output = self.select(functions, constants).output().unwrap();
        succeeds(&output);
        String::from_utf8(output.stdout).unwrap()
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn native(&self, expression: &str, expected: &[u8]) {
        let source = self.write(
            "main.rill",
            &format!("import bindings as b; fn main() {{ {expression} }}"),
        );
        for optimization in ["-O0", "-O2"] {
            let object = self.0.join("fixture object.o");
            succeeds(
                &Command::new("cc")
                    .args(["-std=c11", optimization, "-c"])
                    .arg(self.0.join("api.c"))
                    .arg("-o")
                    .arg(&object)
                    .output()
                    .unwrap(),
            );
            let executable = self.0.join("native program");
            succeeds(
                &Command::new(env!("CARGO_BIN_EXE_rill"))
                    .arg("build")
                    .arg(&source)
                    .arg("-o")
                    .arg(&executable)
                    .args(["--cc", "cc", "--c-arg", "-I", "--c-arg"])
                    .arg(&self.0)
                    .args(["--c-arg", optimization, "--link-arg"])
                    .arg(&object)
                    .output()
                    .unwrap(),
            );
            let output = Command::new(&executable).output().unwrap();
            succeeds(&output);
            assert_eq!(output.stdout, expected, "{optimization}: {expression}");
        }
    }

    fn assert_no_temporary_output(&self) {
        for entry in fs::read_dir(&self.0).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().starts_with(".rill-import-"),
                "left temporary output {name:?}"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Match the selected libclang's resource headers when a separate Clang driver
// is available. An explicitly configured directory takes precedence. The tests
// still try libclang's own defaults when no driver is installed; missing libclang
// or builtin headers is a test failure, not silently reported as coverage.
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

fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn rejects(output: &Output, message: &str) {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.is_empty(), "failed import published stdout");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(message),
        "expected {message:?}, got {stderr}"
    );
}

#[test]
fn imports_only_selected_symbols_and_exact_scalar_typedef_kinds() {
    let fixture = Fixture::new();
    let module = fixture.import(
        &[
            "selected_add",
            "selected_byte",
            "selected_unsigned",
            "selected_wide",
            "selected_float",
            "selected_bool",
            "selected_text",
            "selected_read_i32",
            "selected_null",
            "selected_discard",
        ],
        &["SELECTED_ENUM"],
    );
    for declaration in [
        "@include(\"api.h\");",
        "export extern fn selected_add(arg0: i32, arg1: i32) -> i32;",
        "export extern fn selected_byte(arg0: u8) -> u8;",
        "export extern fn selected_unsigned(arg0: u32) -> u32;",
        "export extern fn selected_wide(arg0: u64) -> u64;",
        "export extern fn selected_float(arg0: f32) -> f32;",
        "export extern fn selected_bool(arg0: bool) -> bool;",
        "export extern fn selected_text(arg0: cstr) -> bool;",
        "export extern fn selected_read_i32(arg0: const_ptr<i32>) -> i32;",
        "export extern fn selected_null() -> ptr<void>;",
        "export extern fn selected_discard(arg0: ptr<void>) -> unit;",
        "export extern const SELECTED_ENUM: i32;",
    ] {
        assert!(
            module.contains(declaration),
            "missing {declaration:?}: {module}"
        );
    }
    assert_eq!(module.matches("export extern fn selected_add(").count(), 1);
    assert!(!module.contains("selected_item"));
    assert!(!module.contains("SELECTED_LARGE"));
    assert!(!module.contains("fn main"));
}

#[test]
fn preserves_pointee_aliases_and_const_inherited_through_typedefs() {
    let fixture = Fixture::new();
    let module = fixture.import(
        &[
            "selected_item",
            "selected_const_item",
            "selected_inspect",
            "selected_inspect_alias",
            "selected_handle",
        ],
        &[],
    );
    for declaration in [
        "selected_item() -> ptr<SelectedAlias>;",
        "selected_const_item() -> const_ptr<SelectedConstAlias>;",
        "selected_inspect(arg0: const_ptr<SelectedAlias>) -> i32;",
        "selected_inspect_alias(arg0: const_ptr<SelectedConstAlias>) -> i32;",
        "selected_handle(arg0: ptr<SelectedAlias>) -> i32;",
    ] {
        assert!(
            module.contains(declaration),
            "missing {declaration:?}: {module}"
        );
    }
    fixture.write("bindings.rill", &module);
    fixture.native("b.selected_handle(b.selected_item())", b"42\n");
    fixture.native("b.selected_inspect_alias(b.selected_const_item())", b"42\n");
}

#[test]
fn generated_bindings_round_trip_through_native_modules_at_o0_and_o2() {
    let fixture = Fixture::new();
    let module = fixture.import(
        &[
            "selected_add",
            "selected_byte",
            "selected_unsigned",
            "selected_wide",
            "selected_float",
            "selected_bool",
            "selected_text",
        ],
        &[
            "SELECTED_ENUM",
            "SELECTED_LARGE",
            "SELECTED_BYTE",
            "SELECTED_UNSIGNED",
            "SELECTED_BOOL",
        ],
    );
    fixture.write("bindings.rill", &module);
    for (expression, expected) in [
        ("b.selected_add(19, 23)", &b"42\n"[..]),
        ("b.selected_byte(b.SELECTED_BYTE)", &b"255\n"[..]),
        (
            "b.selected_unsigned(b.SELECTED_UNSIGNED)",
            &b"4294967295\n"[..],
        ),
        (
            "b.selected_wide(b.SELECTED_LARGE)",
            &b"18446744073709551615\n"[..],
        ),
        ("b.selected_float(f32(2))", &b"2.5\n"[..]),
        (
            "b.selected_bool(b.SELECTED_BOOL) && b.selected_text(c\"Rill\")",
            &b"true\n"[..],
        ),
        ("b.SELECTED_ENUM", &b"42\n"[..]),
    ] {
        fixture.native(expression, expected);
    }
}

#[test]
fn preprocessor_defines_change_selected_signatures_and_constant_types() {
    let fixture = Fixture::new();
    let mut command = fixture.select(&["selected_conditional"], &["SELECTED_CONDITIONAL"]);
    let output = command.output().unwrap();
    succeeds(&output);
    let default = String::from_utf8(output.stdout).unwrap();
    assert!(default.contains("selected_conditional(arg0: i32) -> i32;"));
    assert!(default.contains("SELECTED_CONDITIONAL: i32;"));
    let output = command
        .args(["--c-arg", "-DSELECTED_ALTERNATE=1"])
        .output()
        .unwrap();
    succeeds(&output);
    let alternate = String::from_utf8(output.stdout).unwrap();
    assert!(alternate.contains("selected_conditional(arg0: u32) -> u32;"));
    assert!(alternate.contains("SELECTED_CONDITIONAL: u32;"));
}

#[test]
fn unsupported_function_abis_are_rejected_with_symbol_and_location() {
    let fixture = Fixture::new();
    for symbol in [
        "selected_callback",
        "selected_nested_pointer",
        "selected_tag_only",
        "selected_aggregate",
        "selected_variadic",
        "selected_no_prototype",
        "selected_volatile",
        "selected_restrict",
        "selected_double",
        "selected_signed_byte",
        "selected_signed_wide",
        "selected_enum",
        "selected_return_string",
        "selected_mutable_string",
        "selected_internal",
    ] {
        let output = fixture
            .command("unsupported.h")
            .args(["--function", symbol])
            .output()
            .unwrap();
        rejects(&output, "E-C-IMPORT:");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(symbol), "{symbol}: {error}");
        assert!(error.contains("unsupported.h:"), "{symbol}: {error}");
    }
}

#[test]
#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
fn non_default_calling_conventions_are_rejected() {
    let fixture = Fixture::new();
    let output = fixture
        .command("unsupported.h")
        .args(["--function", "selected_calling_convention"])
        .output()
        .unwrap();
    rejects(&output, "non-default calling convention");
}

#[test]
fn function_macros_runtime_values_and_non_integer_constants_are_rejected() {
    let fixture = Fixture::new();
    for symbol in [
        "SELECTED_FUNCTION_MACRO",
        "SELECTED_FLOAT_CONSTANT",
        "SELECTED_STRING_CONSTANT",
        "SELECTED_RUNTIME_CONSTANT",
        "SELECTED_RUNTIME_VARIABLE",
    ] {
        let output = fixture
            .command("unsupported.h")
            .args(["--constant", symbol])
            .output()
            .unwrap();
        rejects(&output, "E-C-IMPORT:");
        assert!(String::from_utf8_lossy(&output.stderr).contains(symbol));
    }
    let output = fixture
        .command("unsupported.h")
        .args(["--function", "SELECTED_FUNCTION_MACRO"])
        .output()
        .unwrap();
    rejects(&output, "not an ordinary function");
}

#[test]
fn unknown_symbols_and_wrong_selection_kinds_fail_without_partial_stdout() {
    let fixture = Fixture::new();
    for (kind, name, message) in [
        (
            "--function",
            "missing_function",
            "selected function was not found",
        ),
        (
            "--constant",
            "MISSING_CONSTANT",
            "selected constant was not found",
        ),
        ("--function", "SELECTED_ENUM", "not an ordinary function"),
        (
            "--constant",
            "selected_add",
            "only object macros and enum constants",
        ),
    ] {
        let output = fixture
            .select(&["selected_byte"], &[])
            .args([kind, name])
            .output()
            .unwrap();
        rejects(&output, message);
        assert!(String::from_utf8_lossy(&output.stderr).contains(name));
    }
}

#[test]
fn successful_output_replaces_a_sentinel_atomically_in_a_path_with_spaces() {
    let fixture = Fixture::new();
    let destination = fixture.write("generated bindings.rill", "sentinel\n");
    let expected = fixture.import(&["selected_add"], &["SELECTED_ENUM"]);
    let output = fixture
        .select(&["selected_add"], &["SELECTED_ENUM"])
        .arg("-o")
        .arg(&destination)
        .output()
        .unwrap();
    succeeds(&output);
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_to_string(destination).unwrap(), expected);
    fixture.assert_no_temporary_output();
}

#[test]
fn failed_import_preserves_existing_output_and_leaves_no_temporary_file() {
    let fixture = Fixture::new();
    let destination = fixture.write("bindings.rill", "sentinel\n");
    let output = fixture
        .select(&["selected_add", "missing_function"], &[])
        .arg("-o")
        .arg(&destination)
        .output()
        .unwrap();
    rejects(&output, "missing_function");
    assert_eq!(fs::read_to_string(destination).unwrap(), "sentinel\n");
    fixture.assert_no_temporary_output();
}

#[test]
fn malformed_headers_are_rejected_without_touching_output() {
    let fixture = Fixture::new();
    fixture.write("malformed.h", "int32_t broken( ;\n");
    let destination = fixture.write("bindings.rill", "sentinel\n");
    for header in ["malformed.h", "missing header.h"] {
        let output = fixture
            .command(header)
            .args(["--function", "broken"])
            .arg("-o")
            .arg(&destination)
            .output()
            .unwrap();
        rejects(&output, "E-C-IMPORT:");
        assert!(String::from_utf8_lossy(&output.stderr).contains(header));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "sentinel\n");
    }
    fixture.assert_no_temporary_output();
}

#[test]
fn missing_target_compiler_is_an_error_and_does_not_publish_output() {
    let fixture = Fixture::new();
    let destination = fixture.write("bindings.rill", "sentinel\n");
    let output = fixture
        .command_with_compiler(
            "api.h",
            fixture.0.join("compiler does not exist").as_os_str(),
        )
        .args(["--function", "selected_add"])
        .arg("-o")
        .arg(&destination)
        .output()
        .unwrap();
    rejects(&output, "cannot start C compiler");
    assert_eq!(fs::read_to_string(destination).unwrap(), "sentinel\n");
    fixture.assert_no_temporary_output();
}

#[test]
fn compiler_mode_overrides_cannot_bypass_validation_or_replace_output() {
    let fixture = Fixture::new();
    let destination = fixture.write("bindings.rill", "sentinel\n");
    for flag in [
        "-E",
        "-S",
        "-c",
        "-o",
        "-x",
        "-xc++",
        "-std=c++17",
        "-Xclang",
        "-Wp,-P",
        "@response",
        "extra.c",
    ] {
        let output = fixture
            .select(&["selected_add"], &[])
            .args(["--c-arg", flag])
            .arg("-o")
            .arg(&destination)
            .output()
            .unwrap();
        rejects(&output, "unsupported import C argument");
        assert_eq!(
            fs::read_to_string(&destination).unwrap(),
            "sentinel\n",
            "{flag}"
        );
    }
    fixture.assert_no_temporary_output();
}

#[test]
fn failed_atomic_publication_preserves_an_existing_directory() {
    let fixture = Fixture::new();
    let destination = fixture.0.join("output directory");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("sentinel"), "keep\n").unwrap();
    let output = fixture
        .select(&["selected_add"], &[])
        .arg("-o")
        .arg(&destination)
        .output()
        .unwrap();
    rejects(&output, "cannot publish output");
    assert_eq!(
        fs::read_to_string(destination.join("sentinel")).unwrap(),
        "keep\n"
    );
    fixture.assert_no_temporary_output();
}

#[test]
#[cfg(all(target_pointer_width = "64", not(target_os = "windows")))]
fn equal_width_distinct_integer_kinds_are_not_guessed_from_size() {
    let fixture = Fixture::new();
    fixture.write(
        "distinct.h",
        r#"
#include <stdint.h>
typedef __typeof__(_Generic((uint64_t)0,
    unsigned long: (unsigned long long)0,
    default: (unsigned long)0)) SelectedDistinct;
_Static_assert(sizeof(SelectedDistinct) == sizeof(uint64_t), "fixture needs LP64");
_Static_assert(!__builtin_types_compatible_p(SelectedDistinct, uint64_t), "distinct C types");
SelectedDistinct selected_distinct(SelectedDistinct value);
"#,
    );
    let output = fixture
        .command("distinct.h")
        .args(["--function", "selected_distinct"])
        .output()
        .unwrap();
    rejects(&output, "unsupported C type");
    assert!(String::from_utf8_lossy(&output.stderr).contains("selected_distinct"));
}

#[test]
#[cfg(unix)]
fn target_compiler_rechecks_frontend_function_and_constant_types() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.write(
        "target.h",
        r#"
#include <stdint.h>
#ifdef SELECTED_TARGET_DIFFERENT
uint32_t selected_target(uint32_t value);
#define SELECTED_TARGET_CONSTANT UINT32_C(42)
#else
int32_t selected_target(int32_t value);
#define SELECTED_TARGET_CONSTANT INT32_C(42)
#endif
"#,
    );
    // Only the target compiler sees this extra define. This exercises the final
    // semantic check even when the selected frontend successfully imported it.
    let compiler = fixture.write(
        "different target compiler",
        "#!/bin/sh\nexec cc -DSELECTED_TARGET_DIFFERENT=1 \"$@\"\n",
    );
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o700)).unwrap();
    let destination = fixture.write("bindings.rill", "sentinel\n");
    for (kind, symbol) in [
        ("--function", "selected_target"),
        ("--constant", "SELECTED_TARGET_CONSTANT"),
    ] {
        let output = fixture
            .command_with_compiler("target.h", compiler.as_os_str())
            .args([kind, symbol])
            .arg("-o")
            .arg(&destination)
            .output()
            .unwrap();
        rejects(&output, "incompatible");
        assert!(String::from_utf8_lossy(&output.stderr).contains(symbol));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "sentinel\n");
    }
    fixture.assert_no_temporary_output();
}

#[cfg(unix)]
fn executable_script(fixture: &Fixture, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = fixture.write(
        name,
        &format!("#!/bin/sh\nif test \"$1\" = __rill-fixture-ready; then exit 0; fi\n{body}\n"),
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    // Prime macOS executable checks before measuring a worker deadline.
    #[cfg(target_os = "macos")]
    assert!(Command::new(&path)
        .arg("__rill-fixture-ready")
        .status()
        .unwrap()
        .success());
    path
}

#[test]
#[cfg(unix)]
fn importer_deadline_covers_target_compiler_and_cleans_its_workspace() {
    let fixture = Fixture::new();
    let marker = fixture.0.join("workspace");
    let compiler = executable_script(&fixture, "hanging compiler", &format!(
        "printf '%s' \"$TMPDIR\" > '{}'\nprintf artifact > \"$TMPDIR/compiler-artifact\"\nsleep 30 &\nwait", marker.display()
    ));
    let destination = fixture.write("bindings.rill", "sentinel\n");
    let output = fixture
        .command_with_compiler("api.h", compiler.as_os_str())
        .args(["--function", "selected_add", "--timeout-ms", "1000"])
        .arg("-o")
        .arg(&destination)
        .output()
        .unwrap();
    rejects(&output, "import worker timed out after 1000 ms");
    assert_eq!(fs::read_to_string(destination).unwrap(), "sentinel\n");
    let workspace = fs::read_to_string(marker).expect("target compiler must have started");
    assert!(!Path::new(&workspace).exists());
    fixture.assert_no_temporary_output();
}

#[test]
#[cfg(unix)]
fn importer_deadline_also_covers_pkg_config() {
    let fixture = Fixture::new();
    let marker = fixture.0.join("workspace");
    executable_script(
        &fixture,
        "pkg-config",
        &format!(
            "printf '%s' \"$TMPDIR\" > '{}'\nsleep 30 &\nwait",
            marker.display()
        ),
    );
    let mut paths = vec![fixture.0.clone()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let destination = fixture.write("bindings.rill", "sentinel\n");
    let output = fixture
        .select(&["selected_add"], &[])
        .args(["--pkg-config", "fixture", "--timeout-ms", "300"])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .arg("-o")
        .arg(&destination)
        .output()
        .unwrap();
    rejects(&output, "import worker timed out");
    assert_eq!(fs::read_to_string(destination).unwrap(), "sentinel\n");
    let workspace = fs::read_to_string(marker).expect("pkg-config must have started");
    assert!(!Path::new(&workspace).exists());
    fixture.assert_no_temporary_output();
}

#[test]
#[cfg(unix)]
fn sigint_and_sigterm_cancel_import_and_preserve_existing_output() {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let fixture = Fixture::new();
        let marker = fixture.0.join("workspace");
        let survived = fixture.0.join("survived");
        let compiler = executable_script(&fixture, "hanging compiler", &format!(
            "printf '%s' \"$TMPDIR\" > '{}'\nprintf artifact > \"$TMPDIR/compiler-artifact\"\n(sleep 1; printf survived > '{}') &\nwait", marker.display(), survived.display()
        ));
        let destination = fixture.write("bindings.rill", "sentinel\n");
        let mut child = fixture
            .command_with_compiler("api.h", compiler.as_os_str())
            .args(["--function", "selected_add", "--timeout-ms", "10000"])
            .arg("-o")
            .arg(&destination)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !marker.exists() {
            if start.elapsed() > Duration::from_secs(5) || child.try_wait().unwrap().is_some() {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "compiler did not start: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // The test owns this still-running child and sends only the tested signal.
        assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
        let output = child.wait_with_output().unwrap();
        rejects(&output, "import worker cancelled by signal");
        assert_eq!(fs::read_to_string(destination).unwrap(), "sentinel\n");
        let workspace = fs::read_to_string(marker).unwrap();
        assert!(!Path::new(&workspace).exists());
        fixture.assert_no_temporary_output();
        std::thread::sleep(Duration::from_millis(1200));
        assert!(
            !survived.exists(),
            "compiler descendant survived cancellation"
        );
    }
}

#[test]
fn importer_timeout_options_are_bounded_and_not_repeatable() {
    let fixture = Fixture::new();
    for args in [
        vec!["--timeout-ms", "0"],
        vec!["--timeout-ms", "600001"],
        vec!["--timeout-ms", "-1"],
        vec!["--timeout-ms", "invalid"],
        vec!["--timeout-ms", "1", "--timeout-ms", "2"],
    ] {
        let output = fixture
            .select(&["selected_add"], &[])
            .args(args)
            .output()
            .unwrap();
        rejects(&output, "--timeout-ms");
    }
}

#[test]
#[cfg(unix)]
fn isolated_import_preserves_non_utf8_include_paths_and_relative_cwd() {
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let directory = fixture.0.join(OsString::from_vec(b"headers \xff".to_vec()));
    if let Err(error) = fs::create_dir(&directory) {
        if cfg!(target_os = "macos")
            && matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::InvalidInput
            )
        {
            eprintln!("SKIP selected C import non-UTF8 filesystem paths: create_dir rejected the raw-byte filename: {error}");
            return;
        }
        panic!("cannot create non-UTF8 include directory: {error}");
    }
    fs::copy(fixture.0.join("api.h"), directory.join("api.h")).unwrap();
    let compiler = executable_script(&fixture, "local compiler", "exec cc \"$@\"");
    let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
    command
        .current_dir(&fixture.0)
        .args([
            "import-c",
            "--header",
            "api.h",
            "--function",
            "selected_add",
            "--cc",
            "./local compiler",
            "--c-arg",
            "-I",
            "--c-arg",
        ])
        .arg(directory.file_name().unwrap());
    if let Some(directory) = resource_dir() {
        command.arg("--clang-resource-dir").arg(directory);
    }
    let output = command.output().unwrap();
    succeeds(&output);
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("selected_add(arg0: i32, arg1: i32) -> i32"));
    assert!(compiler.is_file());
}

#[test]
#[cfg(unix)]
fn inherited_ignored_sigchld_does_not_lose_worker_ownership() {
    let fixture = Fixture::new();
    let command = fixture.select(&["selected_add"], &[]);
    let output = Command::new("sh")
        .args(["-c", "trap '' CHLD; exec \"$@\"", "rill-ignored-sigchld"])
        .arg(command.get_program())
        .args(command.get_args())
        .output()
        .unwrap();
    succeeds(&output);
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("selected_add(arg0: i32, arg1: i32) -> i32"));
}

#[test]
fn explicit_in_process_library_api_matches_the_isolated_cli() {
    use rill::c_import::{
        selected::{self, Selection},
        Config,
    };
    let fixture = Fixture::new();
    let selection = Selection {
        native: Config {
            includes: vec!["api.h".into()],
            ..Config::default()
        },
        functions: vec!["selected_add".into()],
        constants: vec!["SELECTED_ENUM".into()],
        c_args: vec!["-I".into(), fixture.0.as_os_str().to_owned()],
        resource_dir: resource_dir().map(Path::to_path_buf),
    };
    let module = selected::generate(&selection, OsStr::new("cc"), false).unwrap();
    assert_eq!(
        module,
        fixture.import(&["selected_add"], &["SELECTED_ENUM"])
    );
}
