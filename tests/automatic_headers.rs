#![cfg(all(feature = "c-import", unix))]

use rill::lsp::{analysis, text};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("rill-auto-headers-{}-{name}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("api.h"), "#include <stdint.h>\n#include \"child.h\"\nint32_t api_add(int32_t left, int32_t right);\nint32_t api_log(const char *format, ...);\n#ifdef __OPTIMIZE__\n#define API_OPT ((int32_t)42)\n#else\n#define API_OPT ((int32_t)0)\n#endif\n").unwrap();
        fs::write(path.join("child.h"), "#define API_FLAG ((uint32_t)7)\n").unwrap();
        fs::write(path.join("api.c"), "#include \"api.h\"\nint32_t api_add(int32_t left, int32_t right) { return left + right; }\n").unwrap();
        Self(path)
    }

    fn command(&self, operation: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .arg(operation)
            .arg(self.0.join("main.rill"))
            .args(["--c-arg", "-I", "--c-arg"])
            .arg(&self.0);
        command
    }

    fn run(&self) -> Output {
        self.command("run")
            .arg("--link-arg")
            .arg(self.0.join("api.c"))
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn include_only_modules_auto_export_transitive_declarations_without_source_writes() {
    let fixture = Fixture::new("exports");
    let bindings = "@include(\"api.h\");\n";
    let main = "import native as n; fn main() { let answer = n.api_add(20, 22); if n.API_FLAG == u32(7) { answer } else { 0 } }";
    fs::write(fixture.0.join("native.rill"), bindings).unwrap();
    fs::write(fixture.0.join("main.rill"), main).unwrap();
    succeeds(&fixture.command("check").output().unwrap());
    let result = fixture.run();
    succeeds(&result);
    assert_eq!(result.stdout, b"42\n");
    let binary = fixture.0.join("program");
    succeeds(
        &fixture
            .command("build")
            .arg("-o")
            .arg(&binary)
            .arg("--link-arg")
            .arg(fixture.0.join("api.c"))
            .args(["--c-arg", "-O0"])
            .output()
            .unwrap(),
    );
    assert_eq!(Command::new(binary).output().unwrap().stdout, b"42\n");
    assert_eq!(
        fs::read_to_string(fixture.0.join("native.rill")).unwrap(),
        bindings
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("main.rill")).unwrap(),
        main
    );
}

#[test]
fn unsupported_referenced_apis_fail_and_unknown_names_are_not_fabricated() {
    let fixture = Fixture::new("unsupported");
    for (call, expected) in [
        ("api_log(c\"message\")", "variadic"),
        ("missing_api(1)", "missing_api"),
    ] {
        fs::write(
            fixture.0.join("main.rill"),
            format!("@include(\"api.h\"); fn main() {{ {call} }}"),
        )
        .unwrap();
        let output = fixture.command("check").output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        if call.starts_with("api_log") {
            assert!(
                error.contains("api.h:") && error.contains("E-C-IMPORT"),
                "{error}"
            );
        }
    }
}

#[test]
fn build_snapshots_support_editor_types_and_never_navigate_past_source_end() {
    let fixture = Fixture::new("editor");
    let source = "@include(\"api.h\"); fn main() { let answer = api_add(20, 22); answer }";
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, source).unwrap();
    succeeds(&fixture.command("check").output().unwrap());
    let uri = text::path_to_uri(&entry).unwrap();
    let overlays = BTreeMap::new();
    assert!(analysis::diagnostics(&uri, source, &overlays).is_empty());
    let at = source.find("api_add").unwrap();
    assert!(analysis::definition(&uri, source, at, &overlays).is_none());
    let hover = analysis::hover(&uri, source, at, &overlays).unwrap();
    assert!(hover.to_string().contains("i32"), "{hover}");
    let completions = analysis::completion(&uri, source, at, &overlays);
    assert!(
        completions.iter().any(|item| item["label"] == "api_add"
            && item["detail"].as_str().is_some_and(|d| d.contains("i32"))),
        "{completions:?}"
    );
    let hints = analysis::inlay_hints(&uri, source, 0..source.len(), &overlays);
    assert!(
        hints.iter().any(|hint| hint["label"] == ": i32"),
        "{hints:?}"
    );
    assert!(hints
        .iter()
        .all(|hint| hint["position"]["character"].as_u64().unwrap() <= source.len() as u64));
    fs::write(
        fixture.0.join("child.h"),
        "#define API_FLAG ((uint32_t)8)\n",
    )
    .unwrap();
    assert!(
        !analysis::diagnostics(&uri, source, &overlays).is_empty(),
        "transitive header change must invalidate snapshot"
    );
}

#[test]
fn running_native_code_cannot_bless_stale_bindings_with_changed_header_contents() {
    let fixture = Fixture::new("header-mutation");
    let header = fixture.0.join("api.h");
    fs::write(&header, "#include <stdint.h>\nint32_t api_mutate(void);\n").unwrap();
    fs::write(fixture.0.join("api.c"), "#include \"api.h\"\n#include <stdio.h>\n#include <stdlib.h>\nint32_t api_mutate(void) { FILE *file = fopen(getenv(\"RILL_TEST_HEADER_PATH\"), \"w\"); if (!file) return 1; fputs(\"#include <stdint.h>\\nuint32_t api_mutate(void);\\n\", file); fclose(file); return 42; }\n").unwrap();
    let source = "@include(\"api.h\"); fn main() { api_mutate() }";
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, source).unwrap();
    let output = fixture
        .command("run")
        .arg("--link-arg")
        .arg(fixture.0.join("api.c"))
        .env("RILL_TEST_HEADER_PATH", &header)
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(output.stdout, b"42\n");
    assert!(fs::read_to_string(&header)
        .unwrap()
        .contains("uint32_t api_mutate"));
    let mut graph = rill::modules::load(&entry).unwrap();
    assert!(
        rill::modules::headers::apply_cached(&mut graph)
            .unwrap()
            .is_empty(),
        "a pre-run i32 interface must not be paired with post-run u32 header fingerprints"
    );
    let uri = text::path_to_uri(&entry).unwrap();
    assert!(
        !analysis::diagnostics(&uri, source, &BTreeMap::new()).is_empty(),
        "editor analysis must reject the stale pre-run interface"
    );
}

#[test]
fn automatic_import_uses_the_same_ordered_header_environment_as_final_c_output() {
    let fixture = Fixture::new("header-environment");
    fs::write(
        fixture.0.join("a.h"),
        "#include <stdint.h>\n#define ARG_KIND uint32_t\n",
    )
    .unwrap();
    fs::write(fixture.0.join("b.h"), "#include <stdint.h>\n#ifndef ARG_KIND\n#define ARG_KIND int32_t\n#endif\nARG_KIND env_add(ARG_KIND value);\n").unwrap();
    fs::write(fixture.0.join("a.rill"), "@include(\"a.h\");").unwrap();
    fs::write(fixture.0.join("b.rill"), "@include(\"b.h\");").unwrap();
    fs::write(
        fixture.0.join("main.rill"),
        "import a; import b; fn main() { b.env_add(u32(41)) }",
    )
    .unwrap();
    succeeds(&fixture.command("check").output().unwrap());
}

#[test]
fn an_explicit_clang_resource_directory_is_not_overridden_by_discovery() {
    let fixture = Fixture::new("explicit-resource");
    let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let output = Command::new(compiler)
        .arg("-print-resource-dir")
        .output()
        .unwrap();
    if !output.status.success() {
        eprintln!("SKIP explicit Clang resource directory: selected C compiler does not expose Clang resources");
        return;
    }
    let original = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    let custom = fixture.0.join("custom-resource");
    let include = custom.join("include");
    fs::create_dir_all(&include).unwrap();
    for entry in fs::read_dir(original.join("include")).unwrap() {
        let entry = entry.unwrap();
        std::os::unix::fs::symlink(entry.path(), include.join(entry.file_name())).unwrap();
    }
    fs::write(
        include.join("native_marker.h"),
        "#define NATIVE_MARKER ((int32_t)42)\n",
    )
    .unwrap();
    fs::write(
        fixture.0.join("api.h"),
        "#include <stdint.h>\n#include <native_marker.h>\n",
    )
    .unwrap();
    fs::write(
        fixture.0.join("main.rill"),
        "@include(\"api.h\"); fn main() { NATIVE_MARKER }",
    )
    .unwrap();
    let output = fixture
        .command("run")
        .arg("--c-arg")
        .arg(format!("-resource-dir={}", custom.display()))
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn automatic_calls_in_callable_bodies_use_checked_native_lowering() {
    let fixture = Fixture::new("callable-native");
    fs::write(fixture.0.join("main.rill"), "@include(\"api.h\"); fn main() { let add = fn(value) { api_add(value, 1) }; let twice = add >> add; twice(40) }").unwrap();
    let output = fixture.run();
    succeeds(&output);
    assert_eq!(output.stdout, b"42\n");
}
