use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rill recipe {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir(root.join("native")).unwrap();
        fs::write(
            root.join("src/main.rill"),
            "extern fn answer() -> i32; fn main() { answer() }",
        )
        .unwrap();
        fs::write(
            root.join("native/answer.h"),
            "#include <stdint.h>\nint32_t answer(void);\n",
        )
        .unwrap();
        fs::write(
            root.join("native/answer.c"),
            "#include \"answer.h\"\nint32_t answer(void) { return 42; }\n",
        )
        .unwrap();
        let project = Self(root);
        project.manifest(r#"{"version":2,"name":"recipe","entry":"src/main.rill","native":{"sources":["native/answer.c"],"include_dirs":["native"]}}"#);
        project
    }
    fn manifest(&self, text: &str) {
        fs::write(self.0.join("rill.json"), text).unwrap();
    }
    fn command(&self, action: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command.args(["project", action]).arg(&self.0);
        command
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn project_native_recipes_check_build_run_and_watch_once() {
    let project = Project::new();
    success(&project.command("check").output().unwrap());
    assert!(
        !project.0.join("build").exists(),
        "check must not publish objects or executables"
    );
    for action in ["run", "watch"] {
        let mut command = project.command(action);
        if action == "watch" {
            command.arg("--once");
        }
        let output = command.output().unwrap();
        success(&output);
        assert_eq!(output.stdout, b"42\n");
    }
    success(&project.command("build").output().unwrap());
    let output = Command::new(
        project
            .0
            .join(format!("build/recipe{}", std::env::consts::EXE_SUFFIX)),
    )
    .output()
    .unwrap();
    success(&output);
    assert_eq!(output.stdout, b"42\n");
    fs::write(
        project.0.join("native/answer.c"),
        "#include \"answer.h\"\nint32_t answer(void) { return 43; }\n",
    )
    .unwrap();
    let output = project.command("run").output().unwrap();
    success(&output);
    assert_eq!(output.stdout, b"43\n");
}

#[test]
fn frontend_cache_hits_still_compile_current_native_sources() {
    let project = Project::new();
    fs::write(project.0.join("src/main.rill"), "fn main() { 42 }").unwrap();
    success(
        &project
            .command("build")
            .arg("--cache-status")
            .output()
            .unwrap(),
    );
    fs::write(project.0.join("native/answer.c"), "not valid C\n").unwrap();
    let output = project
        .command("build")
        .arg("--cache-status")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("rill cache: hit"), "{stderr}");
    assert!(stderr.contains("E-PROJECT-NATIVE"), "{stderr}");
    let output = Command::new(
        project
            .0
            .join(format!("build/recipe{}", std::env::consts::EXE_SUFFIX)),
    )
    .output()
    .unwrap();
    success(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn native_objects_precede_source_dependency_libraries() {
    let project = Project::new();
    fs::write(
        project.0.join("src/main.rill"),
        "@link(\"recipe_fixture\"); extern fn answer() -> i32; fn main() { answer() }",
    )
    .unwrap();
    fs::write(
        project.0.join("native/library.c"),
        "int from_library(void) { return 41; }\n",
    )
    .unwrap();
    fs::write(project.0.join("native/answer.c"), "#include \"answer.h\"\nint from_library(void); int32_t answer(void) { return from_library() + 1; }\n").unwrap();
    let object = project.0.join("native/library.o");
    success(
        &Command::new("cc")
            .args(["-std=c11", "-c"])
            .arg(project.0.join("native/library.c"))
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    success(
        &Command::new("ar")
            .arg("crs")
            .arg(project.0.join("native/librecipe_fixture.a"))
            .arg(&object)
            .output()
            .unwrap(),
    );
    let output = project
        .command("run")
        .env("LIBRARY_PATH", project.0.join("native"))
        .output()
        .unwrap();
    success(&output);
    assert_eq!(output.stdout, b"42\n");
}

#[test]
fn invalid_v2_recipes_preserve_strict_v1_and_path_boundaries() {
    let project = Project::new();
    for extra in [
        r#""native":null"#,
        r#""native":{"commands":["echo bad"]}"#,
        r#""native":{"sources":"native/answer.c"}"#,
        r#""native":{"sources":["native/answer.c","native/answer.c"]}"#,
        r#""native":{"sources":["../outside.c"]}"#,
        r#""native":{"sources":["native/answer.h"]}"#,
        r#""native":{"include_dirs":["native/answer.c"]}"#,
        r#""native":{"sources":[],"sources":["native/answer.c"]}"#,
        r#""shaders":false"#,
        r#""shaders":"../outside.json"#,
    ] {
        project.manifest(&format!(
            r#"{{"version":2,"name":"recipe","entry":"src/main.rill",{extra}}}"#
        ));
        let output = project.command("check").output().unwrap();
        assert!(!output.status.success(), "accepted {extra}");
    }
    project.manifest(
        r#"{"version":1,"name":"recipe","entry":"src/main.rill","native":{"sources":[]}}"#,
    );
    assert!(!project.command("check").output().unwrap().status.success());
    project.manifest(r#"{"version":2,"name":"recipe","entry":"src/main.rill","output":"native/answer.c","native":{"sources":["native/answer.c"]}}"#);
    assert!(!project.command("build").output().unwrap().status.success());
}

#[test]
fn recipes_reject_interpretation_saved_config_and_mismatched_entry() {
    let project = Project::new();
    let output = project.command("run").arg("--interpret").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("native project recipes"));
    let output = project
        .command("check")
        .args(["--native-config", "missing.rillcfg", "--cc", "cc"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot be combined with project recipes")
    );
    let other = project.0.join("src/other.rill");
    fs::write(&other, "fn main() { 0 }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .arg("check")
        .arg(&other)
        .arg("--project-root")
        .arg(&project.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source must match"));
}

#[test]
fn shader_recipes_validate_without_publication_and_supply_runtime_asset_paths() {
    let project = Project::new();
    fs::create_dir(project.0.join("shaders")).unwrap();
    fs::write(
        project.0.join("shaders/main.metal"),
        "#include <metal_stdlib>\nusing namespace metal;\nkernel void clear() {}\n",
    )
    .unwrap();
    fs::write(project.0.join("rill.shaders.json"), r#"{"version":1,"output":"build/shaders","shaders":[{"name":"clear","language":"metal","stage":"compute","entry":"clear","source":"shaders/main.metal","target":"msl-source"}]}"#).unwrap();
    project.manifest(r#"{"version":2,"name":"recipe","entry":"src/main.rill","native":{"sources":["native/answer.c"],"include_dirs":["native"]},"shaders":"rill.shaders.json"}"#);
    fs::write(project.0.join("native/answer.c"), "#include \"answer.h\"\n#include <stdlib.h>\n#include <stdio.h>\nint32_t answer(void) { const char *dir = getenv(\"RILL_PROJECT_SHADER_DIR\"); if (!dir || !getenv(\"RILL_PROJECT_ROOT\")) return 0; char path[4096]; if (snprintf(path, sizeof path, \"%s/manifest.json\", dir) >= (int)sizeof path) return 0; FILE *file = fopen(path, \"r\"); if (!file) return 0; fclose(file); return 42; }\n").unwrap();
    success(&project.command("check").output().unwrap());
    assert!(!project.0.join("build").exists());
    for action in ["run", "watch"] {
        let mut command = project.command(action);
        if action == "watch" {
            command.arg("--once");
        }
        let output = command.output().unwrap();
        success(&output);
        assert_eq!(output.stdout, b"42\n");
    }
    fs::write(project.0.join("rill.shaders.json"), "{}\n").unwrap();
    let output = project.command("run").output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(project.0.join("build/shaders/manifest.json").is_file());
}

#[cfg(unix)]
#[test]
fn native_input_symlinks_cannot_escape_the_project() {
    use std::os::unix::fs::symlink;
    let project = Project::new();
    let outside = project
        .0
        .parent()
        .unwrap()
        .join(format!("rill-outside-{}.c", std::process::id()));
    fs::write(&outside, "int answer(void) { return 0; }").unwrap();
    symlink(&outside, project.0.join("native/escape.c")).unwrap();
    project.manifest(r#"{"version":2,"name":"recipe","entry":"src/main.rill","native":{"sources":["native/escape.c"]}}"#);
    let output = project.command("check").output().unwrap();
    fs::remove_file(&outside).unwrap();
    assert!(!output.status.success());
}
