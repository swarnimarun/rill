#![cfg(unix)]
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

// Serialize executable fixture writes with all child launches in this test
// process: another launch must not inherit a still-writable script descriptor.
static FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Fixture(PathBuf, Option<std::sync::MutexGuard<'static, ()>>);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let execution = FIXTURES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = std::env::temp_dir().join(format!(
            "rill native configuration {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory, Some(execution))
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_rill"));
        c.current_dir(&self.0);
        c
    }
    fn clean_outputs(&self) {
        for entry in fs::read_dir(&self.0).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().starts_with(".rill-"),
                "leaked publication temporary {name:?}"
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        // Keep the guard until the directory and all owned children are done.
        let _ = self.1.take();
    }
}
#[cfg(feature = "c-import")]
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn failure(output: &Output, category: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(category),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "partial stdout: {:?}",
        output.stdout
    );
}

#[test]
fn malformed_inspection_and_cli_conflicts_are_read_only() {
    let f = Fixture::new();
    fs::write(f.path("bad.rillcfg"), b"not a configuration").unwrap();
    failure(
        &f.command()
            .args(["import-c", "--inspect-config", "bad.rillcfg"])
            .output()
            .unwrap(),
        "E-C-CONFIG",
    );
    failure(
        &f.command()
            .args([
                "import-c",
                "--inspect-config",
                "bad.rillcfg",
                "--cc",
                "never-execute",
            ])
            .output()
            .unwrap(),
        "E-C-CONFIG",
    );
    for args in [
        vec!["import-c", "--config", "bad.rillcfg", "--config-out", "out"],
        vec!["import-c", "--config-out", "out", "-o", "bindings.rill"],
        vec!["import-c", "--config-out", "out"],
        vec!["import-c", "--config", "bad.rillcfg", "--c-arg", "-I."],
        vec!["check", "missing.rill", "--native-config", "bad.rillcfg"],
        vec![
            "check",
            "missing.rill",
            "--native-config",
            "bad.rillcfg",
            "--cc",
            "cc",
            "--simd",
            "neon",
        ],
        vec![
            "check",
            "missing.rill",
            "--native-config",
            "bad.rillcfg",
            "--cc",
            "cc",
            "--c-arg",
            "-O0",
        ],
        vec![
            "check",
            "missing.rill",
            "--native-config",
            "bad.rillcfg",
            "--native-config",
            "other",
            "--cc",
            "cc",
        ],
    ] {
        failure(&f.command().args(&args).output().unwrap(), "E-C-CONFIG");
    }
    assert!(!f.path("out").exists());
    f.clean_outputs();
}

#[test]
fn checked_snapshot_keeps_the_exact_lowered_sources() {
    let f = Fixture::new();
    let original = "export fn answer() { 42 }\n";
    fs::write(f.path("bindings.rill"), original).unwrap();
    fs::write(
        f.path("main.rill"),
        "import bindings; fn main() { bindings.answer() }",
    )
    .unwrap();
    let snapshot = rill::modules::compile::load_program_snapshot(&f.path("main.rill")).unwrap();
    fs::write(
        f.path("bindings.rill"),
        "invalid replacement after the one load",
    )
    .unwrap();
    assert_eq!(snapshot.source("bindings"), Some(original));
    assert_eq!(snapshot.source("other"), None);
    assert_eq!(
        rill::interpreter::run(snapshot.program())
            .unwrap()
            .to_string(),
        "42"
    );
}

#[cfg(not(feature = "c-import"))]
#[test]
fn unavailable_configuration_does_not_fall_back_to_legacy() {
    let f = Fixture::new();
    fs::write(f.path("out"), "sentinel").unwrap();
    failure(
        &f.command()
            .args([
                "import-c",
                "--config-out",
                "out",
                "--binding-module",
                "bindings",
                "--cc",
                "cc",
            ])
            .output()
            .unwrap(),
        "--features c-import",
    );
    failure(
        &f.command()
            .args([
                "check",
                "missing.rill",
                "--native-config",
                "out",
                "--cc",
                "cc",
            ])
            .output()
            .unwrap(),
        "--features c-import",
    );
    assert_eq!(fs::read(f.path("out")).unwrap(), b"sentinel");
    f.clean_outputs();
}

#[cfg(feature = "c-import")]
mod native {
    use super::*;
    use rill::c_import::configuration::{resolve_explicit_compiler, UnverifiedConfig};
    use std::{
        ffi::{OsStr, OsString},
        os::unix::{
            ffi::OsStringExt,
            fs::{symlink, PermissionsExt},
        },
        path::Path,
        sync::OnceLock,
    };
    fn compiler() -> PathBuf {
        resolve_explicit_compiler(OsStr::new("cc"), false, &std::env::current_dir().unwrap())
            .unwrap()
            .invocation_path
    }
    fn resource() -> Option<&'static Path> {
        static RESOURCE: OnceLock<Option<PathBuf>> = OnceLock::new();
        RESOURCE
            .get_or_init(|| {
                if let Some(path) = std::env::var_os("RILL_TEST_CLANG_RESOURCE_DIR") {
                    return Some(path.into());
                }
                let output =
                    Command::new(std::env::var_os("CLANG_PATH").unwrap_or_else(|| "clang".into()))
                        .arg("-print-resource-dir")
                        .output()
                        .ok()?;
                output
                    .status
                    .success()
                    .then(|| PathBuf::from(String::from_utf8(output.stdout).unwrap().trim()))
            })
            .as_deref()
    }
    fn base(f: &Fixture) -> Command {
        let mut c = f.command();
        c.args([
            "import-c", "--header", "api.h", "--c-arg", "-I", "--c-arg", ".", "--cc",
        ])
        .arg(compiler());
        if let Some(resource) = resource() {
            c.arg("--clang-resource-dir").arg(resource);
        }
        c
    }
    fn header(f: &Fixture) {
        fs::write(f.path("api.h"),"#include <stdint.h>\n#include <stdbool.h>\n#define VALUE INT32_C(41)\nint32_t answer(int32_t value);\n").unwrap();
    }
    fn capture(f: &Fixture) {
        success(
            &base(f)
                .args([
                    "--function",
                    "answer",
                    "--constant",
                    "VALUE",
                    "--config-out",
                    "native.rillcfg",
                    "--binding-module",
                    "bindings",
                ])
                .output()
                .unwrap(),
        );
    }
    fn materialize(f: &Fixture) -> Command {
        let mut c = f.command();
        c.args(["import-c", "--config", "native.rillcfg", "--cc"])
            .arg(compiler())
            .args(["-o", "bindings.rill"]);
        c
    }
    fn consumer(f: &Fixture, mode: &str) -> Command {
        let mut c = f.command();
        c.args([
            mode,
            "main.rill",
            "--native-config",
            "native.rillcfg",
            "--cc",
        ])
        .arg(compiler());
        c
    }
    fn main_source(f: &Fixture) {
        fs::write(
            f.path("main.rill"),
            "import bindings as b; fn main() { b.VALUE + 1 }\n",
        )
        .unwrap();
    }
    fn script(path: &Path, source: &str) {
        fs::write(
            path,
            format!(
                "#!/bin/sh\nif test \"$1\" = __rill-fixture-ready; then exit 0; fi\n{source}\n"
            ),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        // Avoid measuring a newly written script's macOS executable checks as
        // compiler execution. Production command deadlines remain unchanged.
        #[cfg(target_os = "macos")]
        assert!(Command::new(path)
            .arg("__rill-fixture-ready")
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn capture_is_deterministic_materialization_and_every_consumer_are_fresh() {
        let f = Fixture::new();
        header(&f);
        let wrapper = f.path("recording compiler");
        let log = f.path("compiler.log");
        script(
            &wrapper,
            &format!(
                "printf 'check\\n' >> '{}'\nexec '{}' \"$@\"",
                log.display(),
                compiler().display()
            ),
        );
        let mut command = f.command();
        command
            .args([
                "import-c",
                "--header",
                "api.h",
                "--function",
                "answer",
                "--constant",
                "VALUE",
                "--c-arg",
                "-I",
                "--c-arg",
                ".",
                "--cc",
            ])
            .arg(&wrapper)
            .args([
                "--config-out",
                "native.rillcfg",
                "--binding-module",
                "bindings",
            ]);
        if let Some(r) = resource() {
            command.arg("--clang-resource-dir").arg(r);
        }
        success(&command.output().unwrap());
        let first = fs::read(f.path("native.rillcfg")).unwrap();
        success(&command.output().unwrap());
        assert_eq!(first, fs::read(f.path("native.rillcfg")).unwrap());
        let mut materialize = f.command();
        materialize
            .args(["import-c", "--config", "native.rillcfg", "--cc"])
            .arg(&wrapper)
            .args(["-o", "bindings.rill"]);
        success(&materialize.output().unwrap());
        main_source(&f);
        for mode in ["check", "emit-c", "build", "run"] {
            let before = fs::read_to_string(&log).unwrap().lines().count();
            let mut c = f.command();
            c.args([
                mode,
                "main.rill",
                "--native-config",
                "native.rillcfg",
                "--cc",
            ])
            .arg(&wrapper);
            if mode == "build" {
                c.args(["-o", "program"]);
            }
            let output = c.output().unwrap();
            success(&output);
            assert_eq!(
                fs::read_to_string(&log).unwrap().lines().count(),
                before + 2,
                "{mode} must recheck standalone and whole program"
            );
            if mode == "run" {
                assert_eq!(output.stdout, b"42\n");
            }
            if mode == "emit-c" {
                assert!(String::from_utf8(output.stdout)
                    .unwrap()
                    .contains("incompatible target constant VALUE"));
            }
        }
        assert_eq!(
            Command::new(f.path("program")).output().unwrap().stdout,
            b"42\n"
        );
        f.clean_outputs();
    }
    #[test]
    fn profile_replays_relative_paths_after_move_and_from_other_cwd() {
        let root = Fixture::new();
        let f = Fixture(root.path("project"), None);
        fs::create_dir(&f.0).unwrap();
        header(&f);
        capture(&f);
        success(&materialize(&f).output().unwrap());
        main_source(&f);
        let moved = root.path("relocated");
        fs::rename(&f.0, &moved).unwrap();
        let mut c = root.command();
        c.arg("build")
            .arg(moved.join("main.rill"))
            .arg("--native-config")
            .arg(moved.join("native.rillcfg"))
            .arg("--cc")
            .arg(compiler())
            .args(["-o", "caller-output"]);
        success(&c.output().unwrap());
        assert!(root.path("caller-output").is_file());
        assert!(!moved.join("caller-output").exists());
        success(
            &root
                .command()
                .arg("import-c")
                .arg("--config")
                .arg(moved.join("native.rillcfg"))
                .arg("--cc")
                .arg(compiler())
                .args(["-o", "caller-bindings.rill"])
                .output()
                .unwrap(),
        );
        assert_eq!(
            fs::read(root.path("caller-bindings.rill")).unwrap(),
            fs::read(moved.join("bindings.rill")).unwrap()
        );
    }
    #[test]
    fn inspect_never_executes_artifact_compiler_and_drift_never_publishes() {
        let f = Fixture::new();
        header(&f);
        capture(&f);
        success(&materialize(&f).output().unwrap());
        main_source(&f);
        let marker = f.path("marker");
        let executable = f.path("artifact executable");
        script(
            &executable,
            &format!("touch '{}'\nexit 0", marker.display()),
        );
        let path = f.path("native.rillcfg");
        let original = fs::read(&path).unwrap();
        let mut artifact = UnverifiedConfig::decode(&original).unwrap();
        artifact.compiler_assertion.invocation_path = executable;
        fs::write(&path, artifact.encode().unwrap()).unwrap();
        success(
            &f.command()
                .args(["import-c", "--inspect-config", "native.rillcfg"])
                .env("CC", &artifact.compiler_assertion.invocation_path)
                .output()
                .unwrap(),
        );
        failure(&materialize(&f).output().unwrap(), "E-C-CONFIG-DRIFT");
        assert!(!marker.exists());
        fs::write(&path, original).unwrap();
        fs::write(f.path("bindings.rill"), "sentinel bindings").unwrap();
        let original = fs::read_to_string(f.path("api.h")).unwrap();
        for replacement in [
            original.replace("INT32_C(41)", "INT32_C(42)"),
            original.replace("INT32_C(41)", "UINT32_C(41)"),
            original.replace("int32_t answer", "uint32_t answer"),
        ] {
            fs::write(f.path("api.h"), replacement).unwrap();
            failure(&materialize(&f).output().unwrap(), "E-C-CONFIG-DRIFT");
            assert_eq!(
                fs::read(f.path("bindings.rill")).unwrap(),
                b"sentinel bindings"
            );
            f.clean_outputs();
        }
    }
    #[test]
    fn snapshot_bytes_module_name_and_extra_dependencies_are_strict() {
        let f = Fixture::new();
        header(&f);
        capture(&f);
        success(&materialize(&f).output().unwrap());
        main_source(&f);
        let original = fs::read_to_string(f.path("bindings.rill")).unwrap();
        fs::write(
            f.path("bindings.rill"),
            format!("{original}// harmless edit\n"),
        )
        .unwrap();
        failure(
            &consumer(&f, "emit-c").output().unwrap(),
            "E-C-CONFIG-DRIFT",
        );
        fs::write(f.path("bindings.rill"), &original).unwrap();
        fs::write(f.path("other.rill"), &original).unwrap();
        fs::write(
            f.path("main.rill"),
            "import other as b; fn main() { b.VALUE }",
        )
        .unwrap();
        failure(&consumer(&f, "check").output().unwrap(), "E-C-CONFIG-DRIFT");
        fs::write(
            f.path("main.rill"),
            "@include(\"stdint.h\"); import bindings as b; fn main() { b.VALUE }",
        )
        .unwrap();
        failure(&consumer(&f, "check").output().unwrap(), "E-C-CONFIG-DRIFT");
    }
    #[test]
    fn transitive_constant_drift_and_optimization_profiles_are_observed() {
        let f = Fixture::new();
        fs::write(f.path("inner.h"),"#include <stdint.h>\n#ifdef __OPTIMIZE__\n#define VALUE UINT32_C(42)\n#else\n#define VALUE INT32_C(41)\n#endif\n").unwrap();
        fs::write(f.path("api.h"), "#include \"inner.h\"\n").unwrap();
        for (optimization, ty) in [("-O0", "i32"), ("-O2", "u32")] {
            success(
                &base(&f)
                    .args([
                        "--constant",
                        "VALUE",
                        "--c-arg",
                        optimization,
                        "--config-out",
                        "native.rillcfg",
                        "--binding-module",
                        "bindings",
                    ])
                    .output()
                    .unwrap(),
            );
            success(&materialize(&f).output().unwrap());
            assert!(fs::read_to_string(f.path("bindings.rill"))
                .unwrap()
                .contains(&format!("VALUE: {ty}")));
        }
        fs::write(
            f.path("inner.h"),
            "#include <stdint.h>\n#define VALUE UINT32_C(43)\n",
        )
        .unwrap();
        failure(&materialize(&f).output().unwrap(), "E-C-CONFIG-DRIFT");
    }
    #[test]
    fn raw_fixture_runs_at_saved_o0_o2_with_ordered_object_inputs() {
        let f = Fixture::new();
        fs::write(
            f.path("api.h"),
            include_str!("fixtures/c_import_selected/api.h"),
        )
        .unwrap();
        fs::write(
            f.path("api.c"),
            include_str!("fixtures/c_import_selected/api.c"),
        )
        .unwrap();
        for optimization in ["-O0", "-O2"] {
            let object = f.path("fixture object.o");
            success(
                &Command::new(compiler())
                    .args(["-std=c11", optimization, "-c"])
                    .arg(f.path("api.c"))
                    .arg("-o")
                    .arg(&object)
                    .output()
                    .unwrap(),
            );
            success(
                &base(&f)
                    .args([
                        "--function",
                        "selected_item",
                        "--function",
                        "selected_handle",
                        "--function",
                        "selected_null",
                        "--function",
                        "selected_byte",
                        "--constant",
                        "SELECTED_BYTE",
                        "--c-arg",
                        optimization,
                        "--link-arg",
                    ])
                    .arg(&object)
                    .args([
                        "--config-out",
                        "native.rillcfg",
                        "--binding-module",
                        "bindings",
                    ])
                    .output()
                    .unwrap(),
            );
            success(&materialize(&f).output().unwrap());
            fs::write(f.path("main.rill"),"import bindings as b; fn main() { let item = b.selected_item(); if b.selected_handle(item) == 42 && is_null(b.selected_null()) && b.selected_byte(b.SELECTED_BYTE) == u8(255) { 42 } else { 0 } }").unwrap();
            let out = consumer(&f, "run").output().unwrap();
            success(&out);
            assert_eq!(out.stdout, b"42\n");
        }
    }
    #[test]
    fn non_utf8_include_and_compiler_paths_roundtrip_without_loss() {
        let f = Fixture::new();
        let include = f.0.join(OsString::from_vec(b"headers \xff".to_vec()));
        if let Err(error) = fs::create_dir(&include) {
            #[cfg(target_os = "macos")]
            {
                const MACOS_EPERM: i32 = 1;
                const MACOS_EILSEQ: i32 = 92;
                if matches!(error.raw_os_error(), Some(MACOS_EPERM | MACOS_EILSEQ)) {
                    eprintln!("SKIP native configuration non-UTF8 filesystem paths: create_dir rejected the raw-byte filename: {error}");
                    return;
                }
            }
            panic!("cannot create non-UTF8 include directory: {error}");
        }
        fs::write(
            include.join("api.h"),
            "#include <stdint.h>\n#define VALUE INT32_C(42)\n",
        )
        .unwrap();
        let cc = f.0.join(OsString::from_vec(b"compiler \xfe".to_vec()));
        symlink(compiler(), &cc).unwrap();
        let mut c = f.command();
        c.args([
            "import-c",
            "--header",
            "api.h",
            "--constant",
            "VALUE",
            "--c-arg",
            "-I",
            "--c-arg",
        ])
        .arg(&include)
        .arg("--cc")
        .arg(&cc)
        .args([
            "--config-out",
            "native.rillcfg",
            "--binding-module",
            "bindings",
        ]);
        if let Some(r) = resource() {
            c.arg("--clang-resource-dir").arg(r);
        }
        success(&c.output().unwrap());
        success(
            &f.command()
                .args(["import-c", "--config", "native.rillcfg", "--cc"])
                .arg(&cc)
                .args(["-o", "bindings.rill"])
                .output()
                .unwrap(),
        );
        let inspected = f
            .command()
            .args(["import-c", "--inspect-config", "native.rillcfg"])
            .output()
            .unwrap();
        success(&inspected);
        let text = String::from_utf8(inspected.stdout).unwrap();
        assert!(text.contains("\\xff") && text.contains("\\xfe"));
        assert!(!text.contains('\u{fffd}'));
    }
    #[test]
    fn package_order_is_preserved_and_revalidated_before_reuse() {
        let f = Fixture::new();
        header(&f);
        let tools = f.path("tools");
        fs::create_dir(&tools).unwrap();
        let pkg = tools.join("pkg-config");
        script(
            &pkg,
            "case \"$1\" in --cflags) printf '%s\\n' '-I .';; --libs) cat libraries;; esac",
        );
        fs::write(f.path("libraries"), "-la -lb -la").unwrap();
        let path = std::env::join_paths(
            std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        success(
            &base(&f)
                .args([
                    "--function",
                    "answer",
                    "--pkg-config",
                    "test",
                    "--link",
                    "a",
                    "--config-out",
                    "native.rillcfg",
                    "--binding-module",
                    "bindings",
                ])
                .env("PATH", &path)
                .output()
                .unwrap(),
        );
        let artifact = UnverifiedConfig::read(&f.path("native.rillcfg")).unwrap();
        assert_eq!(
            artifact.expected_resolution.link_args,
            vec![OsString::from("-la"), "-lb".into(), "-la".into()]
        );
        success(&materialize(&f).env("PATH", &path).output().unwrap());
        fs::write(f.path("libraries"), "-lb -la -la").unwrap();
        failure(
            &materialize(&f).env("PATH", &path).output().unwrap(),
            "E-C-CONFIG-DRIFT",
        );
        fs::write(f.path("libraries"), "-E").unwrap();
        failure(
            &materialize(&f).env("PATH", &path).output().unwrap(),
            "E-C-CONFIG",
        );
    }
    #[test]
    fn strict_bypass_timeout_and_target_failure_keep_outputs_unchanged() {
        let f = Fixture::new();
        header(&f);
        fs::write(f.path("native.rillcfg"), "sentinel profile").unwrap();
        for flag in [
            "-E",
            "-Xclang",
            "-D_Static_assert(x,y)=",
            "-D__builtin_types_compatible_p(a,b)=1",
            "-UCHAR_BIT",
        ] {
            failure(
                &base(&f)
                    .args([
                        "--constant",
                        "VALUE",
                        "--c-arg",
                        flag,
                        "--config-out",
                        "native.rillcfg",
                        "--binding-module",
                        "bindings",
                    ])
                    .output()
                    .unwrap(),
                "E-C-CONFIG",
            );
            assert_eq!(
                fs::read(f.path("native.rillcfg")).unwrap(),
                b"sentinel profile"
            );
        }
        for flag in [
            "-E",
            "-o",
            "@response",
            "-Xlinker",
            "-Wl,-plugin,x",
            "extra.c",
        ] {
            failure(
                &base(&f)
                    .args([
                        "--constant",
                        "VALUE",
                        "--link-arg",
                        flag,
                        "--config-out",
                        "native.rillcfg",
                        "--binding-module",
                        "bindings",
                    ])
                    .output()
                    .unwrap(),
                "E-C-CONFIG",
            );
            assert_eq!(
                fs::read(f.path("native.rillcfg")).unwrap(),
                b"sentinel profile"
            );
        }
        let slow = f.path("slow compiler");
        script(&slow, "sleep 30");
        let mut c = f.command();
        c.args([
            "import-c",
            "--header",
            "api.h",
            "--constant",
            "VALUE",
            "--c-arg",
            "-I",
            "--c-arg",
            ".",
            "--cc",
        ])
        .arg(slow)
        .args([
            "--config-out",
            "native.rillcfg",
            "--binding-module",
            "bindings",
            "--timeout-ms",
            "100",
        ]);
        if let Some(r) = resource() {
            c.arg("--clang-resource-dir").arg(r);
        }
        failure(&c.output().unwrap(), "timed out");
        assert_eq!(
            fs::read(f.path("native.rillcfg")).unwrap(),
            b"sentinel profile"
        );
        capture(&f);
        success(&materialize(&f).output().unwrap());
        main_source(&f);
        fs::write(f.path("program"), "sentinel executable").unwrap();
        fs::write(
            f.path("main.rill"),
            "import bindings as b; fn main() { b.answer(1) }",
        )
        .unwrap();
        failure(
            &consumer(&f, "build")
                .args(["-o", "program"])
                .output()
                .unwrap(),
            "C compiler failed",
        );
        assert_eq!(fs::read(f.path("program")).unwrap(), b"sentinel executable");
        f.clean_outputs();
    }
    #[test]
    fn configured_sdl_game_runs_with_saved_o0_and_o2_profiles() {
        let f = Fixture::new();
        let available = Command::new("pkg-config")
            .args(["--exists", "sdl3"])
            .status();
        if !matches!(available, Ok(status) if status.success()) {
            eprintln!(
                "SKIP configured SDL3 native test: pkg-config and SDL3 development files required"
            );
            return;
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = fs::read_to_string(root.join("examples/sdl3-direct/main.rill"))
            .unwrap()
            .replace("frame < 900", "frame < 3")
            .replace("SDL_Delay(u32(16))", "SDL_Delay(u32(0))");
        assert!(source.contains("sdl.SDL_HasEvent(u32(sdl.SDL_EVENT_QUIT))"));
        fs::write(f.path("main.rill"), source).unwrap();
        for optimization in ["-O0", "-O2"] {
            let mut capture = f.command();
            capture
                .args([
                    "import-c",
                    "--header",
                    "SDL3/SDL.h",
                    "--pkg-config",
                    "sdl3",
                    "--link",
                    "SDL3",
                    "--cc",
                ])
                .arg(compiler())
                .args([
                    "--c-arg",
                    optimization,
                    "--config-out",
                    "native.rillcfg",
                    "--binding-module",
                    "sdl",
                ]);
            for name in ["SDL_INIT_VIDEO", "SDL_EVENT_QUIT"] {
                capture.args(["--constant", name]);
            }
            for name in [
                "SDL_Init",
                "SDL_CreateWindow",
                "SDL_CreateRenderer",
                "SDL_PumpEvents",
                "SDL_HasEvent",
                "SDL_GetMouseState",
                "SDL_SetRenderDrawColor",
                "SDL_RenderClear",
                "SDL_RenderLine",
                "SDL_RenderPresent",
                "SDL_Delay",
                "SDL_DestroyRenderer",
                "SDL_DestroyWindow",
                "SDL_Quit",
            ] {
                capture.args(["--function", name]);
            }
            if let Some(resource) = resource() {
                capture.arg("--clang-resource-dir").arg(resource);
            }
            success(&capture.output().unwrap());
            success(
                &f.command()
                    .args(["import-c", "--config", "native.rillcfg", "--cc"])
                    .arg(compiler())
                    .args(["-o", "sdl.rill"])
                    .output()
                    .unwrap(),
            );
            let output = consumer(&f, "run")
                .env("SDL_VIDEODRIVER", "dummy")
                .output()
                .unwrap();
            success(&output);
            assert_eq!(output.stdout, b"0\n");
        }
        f.clean_outputs();
    }

    #[test]
    fn runtime_cwd_is_not_the_saved_compiler_directory() {
        let root = Fixture::new();
        let f = Fixture(root.path("project"), None);
        fs::create_dir(&f.0).unwrap();
        fs::write(
            f.path("api.h"),
            "#include <stdint.h>\nint32_t asset(void);\n",
        )
        .unwrap();
        fs::write(f.path("api.c"), "#include <stdio.h>\n#include <stdint.h>\nint32_t asset(void) { FILE *f = fopen(\"runtime.asset\", \"rb\"); if (!f) return 0; fclose(f); return 42; }\n").unwrap();
        let object = f.path("asset.o");
        success(
            &Command::new(compiler())
                .args(["-std=c11", "-c"])
                .arg(f.path("api.c"))
                .arg("-o")
                .arg(&object)
                .output()
                .unwrap(),
        );
        success(
            &base(&f)
                .args(["--function", "asset", "--link-arg"])
                .arg(&object)
                .args([
                    "--config-out",
                    "native.rillcfg",
                    "--binding-module",
                    "bindings",
                ])
                .output()
                .unwrap(),
        );
        success(&materialize(&f).output().unwrap());
        fs::write(
            f.path("main.rill"),
            "import bindings; fn main() { bindings.asset() }",
        )
        .unwrap();
        fs::write(root.path("runtime.asset"), "runtime basis").unwrap();
        let output = root
            .command()
            .arg("run")
            .arg(f.path("main.rill"))
            .arg("--native-config")
            .arg(f.path("native.rillcfg"))
            .arg("--cc")
            .arg(compiler())
            .output()
            .unwrap();
        success(&output);
        assert_eq!(output.stdout, b"42\n");
    }

    #[test]
    fn configured_cancellation_keeps_existing_profile_and_kills_descendants() {
        use std::{
            process::Stdio,
            time::{Duration, Instant},
        };
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let f = Fixture::new();
            header(&f);
            let marker = f.path("workspace");
            let survived = f.path("survived");
            let cc = f.path("waiting compiler");
            script(
                &cc,
                &format!(
                    "printf '%s' \"$TMPDIR\" > '{}'\n(sleep 1; printf survived > '{}') &\nwait",
                    marker.display(),
                    survived.display()
                ),
            );
            fs::write(f.path("native.rillcfg"), "sentinel profile").unwrap();
            let mut command = f.command();
            command
                .args([
                    "import-c",
                    "--header",
                    "api.h",
                    "--constant",
                    "VALUE",
                    "--c-arg",
                    "-I",
                    "--c-arg",
                    ".",
                    "--cc",
                ])
                .arg(&cc)
                .args([
                    "--config-out",
                    "native.rillcfg",
                    "--binding-module",
                    "bindings",
                    "--timeout-ms",
                    "10000",
                ]);
            if let Some(resource) = resource() {
                command.arg("--clang-resource-dir").arg(resource);
            }
            let mut child = command
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
                        "configured target did not start: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            // The fixture owns this live CLI child and sends only the tested signal.
            assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
            failure(&child.wait_with_output().unwrap(), "cancelled by signal");
            assert_eq!(
                fs::read(f.path("native.rillcfg")).unwrap(),
                b"sentinel profile"
            );
            let workspace = fs::read_to_string(marker).unwrap();
            assert!(!Path::new(&workspace).exists());
            f.clean_outputs();
            std::thread::sleep(Duration::from_millis(1200));
            assert!(!survived.exists());
        }
    }

    #[test]
    fn publication_directory_failure_and_concurrent_capture_are_whole_files() {
        let f = Fixture::new();
        header(&f);
        fs::create_dir(f.path("native.rillcfg")).unwrap();
        fs::write(f.path("native.rillcfg/sentinel"), "keep").unwrap();
        failure(
            &base(&f)
                .args([
                    "--constant",
                    "VALUE",
                    "--config-out",
                    "native.rillcfg",
                    "--binding-module",
                    "bindings",
                ])
                .output()
                .unwrap(),
            "cannot publish output",
        );
        assert_eq!(
            fs::read(f.path("native.rillcfg/sentinel")).unwrap(),
            b"keep"
        );
        f.clean_outputs();
        let mut children = Vec::new();
        for optimization in ["-O0", "-O2"] {
            children.push(
                base(&f)
                    .args([
                        "--constant",
                        "VALUE",
                        "--c-arg",
                        optimization,
                        "--config-out",
                        "concurrent.rillcfg",
                        "--binding-module",
                        "bindings",
                    ])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for child in children {
            success(&child.wait_with_output().unwrap());
        }
        let artifact = UnverifiedConfig::read(&f.path("concurrent.rillcfg")).unwrap();
        assert!(
            artifact.selection.c_args.contains(&OsString::from("-O0"))
                || artifact.selection.c_args.contains(&OsString::from("-O2"))
        );
        f.clean_outputs();
    }
}
