use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "rill shader test {} {}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("shaders")).unwrap();
        Self(path)
    }
    fn command(&self, action: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .current_dir(&self.0)
            .args(["shader", action])
            .env_remove("RILL_TOOLCHAIN_ROOT")
            .env_remove("RILL_SLANGC")
            .env_remove("SLANGC")
            .env_remove("RILL_GLSLANG_VALIDATOR")
            .env_remove("RILL_SPIRV_VAL");
        command
    }
    fn manifest(&self, shaders: Value) {
        fs::write(
            self.0.join("rill.shaders.json"),
            serde_json::to_vec(&json!({"version":1,"output":"build/shaders","shaders":shaders}))
                .unwrap(),
        )
        .unwrap();
    }
    fn metal(&self) {
        fs::write(self.0.join("shaders/mesh.metal"), "#include <metal_stdlib>\n#include \"common.inc\"\nusing namespace metal;\nvertex float4 vertex_main(uint id [[vertex_id]]) { return float4(SCALE); }\n").unwrap();
        fs::write(self.0.join("shaders/common.inc"), "#define SCALE 1.0\n").unwrap();
        self.manifest(json!([{ "name":"mesh", "source":"shaders/mesh.metal", "language":"metal", "stage":"vertex", "entry":"vertex_main", "target":"msl-source" }]));
    }
    fn output(&self) -> PathBuf {
        self.0.join("build/shaders")
    }
}
impl Drop for Tree {
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
fn failure(output: &Output, expected: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn metadata(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap()
}
fn available(name: &str, flag: &str) -> bool {
    let available = Command::new(name)
        .arg(flag)
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        eprintln!("SKIP: real shader tool {name} unavailable");
    }
    available
}

#[test]
fn inspect_is_typed_and_does_not_invoke_tools() {
    let tree = Tree::new();
    tree.metal();
    success(
        &tree
            .command("inspect")
            .env("RILL_SLANGC", tree.0.join("missing"))
            .env("RILL_GLSLANG_VALIDATOR", tree.0.join("missing"))
            .output()
            .unwrap(),
    );
    assert!(!tree.output().exists());
    let path = tree.0.join("rill.shaders.json");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        original.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
    )
    .unwrap();
    failure(
        &tree.command("inspect").output().unwrap(),
        "duplicate field",
    );
    fs::write(&path, original.replace("msl-source", "msl")).unwrap();
    failure(
        &tree.command("inspect").output().unwrap(),
        "unsupported language/target",
    );
    fs::write(
        &path,
        original.replace("shaders/mesh.metal", "../mesh.metal"),
    )
    .unwrap();
    failure(
        &tree.command("inspect").output().unwrap(),
        "without traversal",
    );
}

#[test]
fn explicit_metal_bundle_tracks_includes_and_requires_runtime_validation() {
    let tree = Tree::new();
    tree.metal();
    let checked = tree.command("check").output().unwrap();
    success(&checked);
    assert!(String::from_utf8_lossy(&checked.stdout).contains("runtime validation required"));
    assert!(!tree.output().exists());
    success(&tree.command("build").output().unwrap());
    let first = metadata(&tree.output());
    assert_eq!(
        first["artifacts"][0]["dependencies"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let text = fs::read_to_string(tree.output().join("mesh.metal")).unwrap();
    assert!(text.contains("#line 1 \"shaders/common.inc\""));
    assert!(text.contains("#include <metal_stdlib>"));
    assert!(!text.contains("#include \"common.inc\""));
    fs::write(tree.0.join("shaders/common.inc"), "#define SCALE 2.0\n").unwrap();
    success(&tree.command("build").output().unwrap());
    assert_ne!(
        first["artifacts"][0]["sha256"],
        metadata(&tree.output())["artifacts"][0]["sha256"]
    );
    let previous = fs::read(tree.output().join("mesh.metal")).unwrap();
    fs::write(
        tree.0.join("shaders/common.inc"),
        "#include \"mesh.metal\"\n",
    )
    .unwrap();
    failure(&tree.command("build").output().unwrap(), "recursive");
    assert_eq!(
        fs::read(tree.output().join("mesh.metal")).unwrap(),
        previous
    );
}

#[test]
fn bundle_does_not_expand_comments_or_silently_guess_macro_includes() {
    let tree = Tree::new();
    tree.metal();
    fs::write(tree.0.join("shaders/mesh.metal"), "/*\n#include \"absent.inc\"\n*/\n// #include \"absent.inc\"\n#include <metal_stdlib>\nvertex float4 vertex_main() { return float4(1); }\n").unwrap();
    success(&tree.command("build").output().unwrap());
    fs::write(
        tree.0.join("shaders/mesh.metal"),
        "#define FILE \"common.inc\"\n#include FILE\n",
    )
    .unwrap();
    failure(
        &tree.command("build").output().unwrap(),
        "literal local includes",
    );
}

#[test]
fn output_ownership_rejects_unknown_files() {
    let tree = Tree::new();
    tree.metal();
    fs::create_dir_all(tree.output()).unwrap();
    fs::write(tree.output().join("notes.txt"), "keep this").unwrap();
    failure(&tree.command("build").output().unwrap(), "unowned output");
    fs::remove_dir_all(tree.output()).unwrap();
    success(&tree.command("build").output().unwrap());
    fs::write(tree.output().join("notes.txt"), "keep this").unwrap();
    failure(
        &tree.command("build").output().unwrap(),
        "unknown output entry",
    );
    assert_eq!(
        fs::read_to_string(tree.output().join("notes.txt")).unwrap(),
        "keep this"
    );
}

#[test]
fn real_slang_compiles_spirv_and_metal_with_reflection_and_dependency_hashes() {
    if !available("slangc", "-version") || !available("spirv-val", "--version") {
        return;
    }
    let tree = Tree::new();
    fs::write(
        tree.0.join("shaders/common.slangh"),
        "float4 position_value() { return float4(0.0, 0.0, 0.0, 1.0); }\n",
    )
    .unwrap();
    fs::write(tree.0.join("shaders/mesh.slang"), "#include \"common.slangh\"\n[shader(\"vertex\")] float4 vertex_main(uint id: SV_VertexID):SV_Position { return position_value(); }\n").unwrap();
    tree.manifest(json!([
      {"name":"mesh_spv","source":"shaders/mesh.slang","language":"slang","stage":"vertex","entry":"vertex_main","target":"spirv"},
      {"name":"mesh_msl","source":"shaders/mesh.slang","language":"slang","stage":"vertex","entry":"vertex_main","target":"msl"}
    ]));
    success(&tree.command("check").output().unwrap());
    assert!(!tree.output().exists());
    success(&tree.command("build").output().unwrap());
    let first = metadata(&tree.output());
    assert_eq!(first["artifacts"].as_array().unwrap().len(), 2);
    assert_eq!(
        first["artifacts"][0]["dependencies"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        &fs::read(tree.output().join("mesh_spv.spv")).unwrap()[..4],
        &[3, 2, 35, 7]
    );
    assert!(fs::read_to_string(tree.output().join("mesh_msl.metal"))
        .unwrap()
        .contains("vertex_main"));
    let reflection: Value =
        serde_json::from_slice(&fs::read(tree.output().join("mesh_spv.reflection.json")).unwrap())
            .unwrap();
    assert_eq!(reflection["entryPoints"][0]["name"], "vertex_main");
    fs::write(
        tree.0.join("shaders/common.slangh"),
        "float4 position_value() { return float4(0.5, 0.0, 0.0, 1.0); }\n",
    )
    .unwrap();
    success(&tree.command("build").output().unwrap());
    assert_ne!(
        first["artifacts"][0]["sha256"],
        metadata(&tree.output())["artifacts"][0]["sha256"]
    );
    let previous = fs::read(tree.output().join("mesh_spv.spv")).unwrap();
    fs::write(
        tree.0.join("shaders/common.slangh"),
        "float4 position_value() { return missing_identifier; }\n",
    )
    .unwrap();
    let failed = tree.command("build").output().unwrap();
    failure(&failed, "missing_identifier");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("common.slangh"));
    assert_eq!(
        fs::read(tree.output().join("mesh_spv.spv")).unwrap(),
        previous
    );
}

#[test]
fn real_glsl_and_hlsl_compile_and_spirv_validator_is_mandatory() {
    if !available("glslangValidator", "--version") || !available("spirv-val", "--version") {
        return;
    }
    let tree = Tree::new();
    fs::write(
        tree.0.join("shaders/mesh.vert"),
        "#version 450\nvoid main() { gl_Position = vec4(0, 0, 0, 1); }\n",
    )
    .unwrap();
    fs::write(
        tree.0.join("shaders/mesh.hlsl"),
        "float4 vertex_main(uint id: SV_VertexID):SV_Position { return float4(0, 0, 0, 1); }\n",
    )
    .unwrap();
    tree.manifest(json!([
      {"name":"glsl","source":"shaders/mesh.vert","language":"glsl","stage":"vertex","entry":"main","target":"spirv"},
      {"name":"hlsl","source":"shaders/mesh.hlsl","language":"hlsl","stage":"vertex","entry":"vertex_main","target":"spirv"}
    ]));
    success(&tree.command("build").output().unwrap());
    let previous = fs::read(tree.output().join("glsl.spv")).unwrap();
    failure(
        &tree
            .command("build")
            .env("RILL_SPIRV_VAL", tree.0.join("no-validator"))
            .output()
            .unwrap(),
        "cannot run",
    );
    assert_eq!(fs::read(tree.output().join("glsl.spv")).unwrap(), previous);
}

#[cfg(unix)]
fn executable(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn compiler_failure_and_invalid_artifacts_preserve_previous_build() {
    let tree = Tree::new();
    tree.metal();
    success(&tree.command("build").output().unwrap());
    let previous = fs::read(tree.output().join("mesh.metal")).unwrap();
    fs::write(
        tree.0.join("shaders/mesh.vert"),
        "this is passed only to the selected fake tool",
    )
    .unwrap();
    tree.manifest(json!([{"name":"mesh","source":"shaders/mesh.vert","language":"glsl","stage":"vertex","entry":"main","target":"spirv"}]));
    let compiler = tree.0.join("compiler $(no shell)");
    executable(
        &compiler,
        "#!/bin/sh\necho 'deliberate selected compiler failure' >&2\nexit 23\n",
    );
    failure(
        &tree
            .command("build")
            .env("RILL_GLSLANG_VALIDATOR", &compiler)
            .output()
            .unwrap(),
        "deliberate selected compiler failure",
    );
    assert_eq!(
        fs::read(tree.output().join("mesh.metal")).unwrap(),
        previous
    );
    executable(&compiler, "#!/bin/sh\nsource=$1\nshift\nwhile [ \"$#\" -gt 0 ]; do\n case \"$1\" in -o) out=$2; shift 2;; --depfile) dep=$2; shift 2;; *) shift;; esac\ndone\nescaped=$(printf '%s' \"$source\" | sed 's/ /\\\\ /g')\nprintf '%s: %s\\n' \"$out\" \"$escaped\" > \"$dep\"\nprintf 'invalid bytes' > \"$out\"\n");
    let validator = tree.0.join("validator");
    executable(&validator, "#!/bin/sh\nexit 0\n");
    failure(
        &tree
            .command("build")
            .env("RILL_GLSLANG_VALIDATOR", &compiler)
            .env("RILL_SPIRV_VAL", &validator)
            .output()
            .unwrap(),
        "SPIR-V must contain",
    );
    assert_eq!(
        fs::read(tree.output().join("mesh.metal")).unwrap(),
        previous
    );
}

#[cfg(unix)]
#[test]
fn symlinked_sources_includes_and_output_ancestors_are_rejected() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    tree.metal();
    fs::rename(tree.0.join("shaders/common.inc"), tree.0.join("actual.inc")).unwrap();
    symlink("../actual.inc", tree.0.join("shaders/common.inc")).unwrap();
    failure(
        &tree.command("build").output().unwrap(),
        "cannot use symlinks",
    );
    fs::remove_file(tree.0.join("shaders/common.inc")).unwrap();
    fs::write(tree.0.join("shaders/common.inc"), "#define SCALE 1").unwrap();
    if tree.0.join("build").exists() {
        fs::remove_dir_all(tree.0.join("build")).unwrap();
    }
    fs::create_dir(tree.0.join("elsewhere")).unwrap();
    symlink("elsewhere", tree.0.join("build")).unwrap();
    failure(&tree.command("build").output().unwrap(), "real directory");
}

#[test]
fn local_metal_includes_preserve_cross_line_comments() {
    let tree = Tree::new();
    tree.metal();
    fs::write(tree.0.join("shaders/mesh.metal"), "/* before\n*/ #include \"common.inc\" /* after\nend */\nvertex float4 vertex_main() { return float4(SCALE); }\n").unwrap();
    success(&tree.command("build").output().unwrap());
    let bundled = fs::read_to_string(tree.output().join("mesh.metal")).unwrap();
    assert!(bundled.contains("/* before\n*/ \n#line 1"));
    assert!(bundled.contains("/* after\nend */"));
    let include = bundled.find("#define SCALE").unwrap();
    assert!(bundled[..include].contains("*/"));
    assert!(bundled[include..].contains("/* after"));
}

#[test]
fn real_slang_tracks_imported_module_sources() {
    if !available("slangc", "-version") || !available("spirv-val", "--version") {
        return;
    }
    let tree = Tree::new();
    fs::create_dir(tree.0.join("shaders/modules")).unwrap();
    fs::write(
        tree.0.join("shaders/modules/positions.slang"),
        "public float4 position_value() { return float4(0, 0, 0, 1); }\n",
    )
    .unwrap();
    fs::write(tree.0.join("shaders/mesh.slang"), "import positions;\n[shader(\"vertex\")] float4 vertex_main(uint id: SV_VertexID):SV_Position { return position_value(); }\n").unwrap();
    tree.manifest(json!([{"name":"mesh","source":"shaders/mesh.slang","language":"slang","stage":"vertex","entry":"vertex_main","target":"spirv","include_dirs":["shaders/modules"]}]));
    success(&tree.command("build").output().unwrap());
    let first = metadata(&tree.output());
    assert!(first["artifacts"][0]["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value["path"] == "shaders/modules/positions.slang"));
    fs::write(
        tree.0.join("shaders/modules/positions.slang"),
        "public float4 position_value() { return float4(0.25, 0, 0, 1); }\n",
    )
    .unwrap();
    success(&tree.command("build").output().unwrap());
    assert_ne!(
        first["artifacts"][0]["sha256"],
        metadata(&tree.output())["artifacts"][0]["sha256"]
    );
}

#[test]
fn real_glsl_treats_shell_characters_in_source_names_as_data() {
    if !available("glslangValidator", "--version") || !available("spirv-val", "--version") {
        return;
    }
    let tree = Tree::new();
    let source = "shaders/mesh $(touch escaped).vert";
    fs::write(
        tree.0.join(source),
        "#version 450\nvoid main() { gl_Position = vec4(0, 0, 0, 1); }\n",
    )
    .unwrap();
    tree.manifest(json!([{"name":"mesh","source":source,"language":"glsl","stage":"vertex","entry":"main","target":"spirv"}]));
    success(&tree.command("build").output().unwrap());
    assert!(!tree.0.join("escaped").exists());
    assert_eq!(
        metadata(&tree.output())["artifacts"][0]["dependencies"][0]["path"],
        source
    );
}

#[cfg(unix)]
#[test]
fn relative_tool_overrides_are_resolved_before_changing_compiler_cwd() {
    let tree = Tree::new();
    fs::write(tree.0.join("shaders/mesh.vert"), "source").unwrap();
    tree.manifest(json!([{"name":"mesh","source":"shaders/mesh.vert","language":"glsl","stage":"vertex","entry":"main","target":"spirv"}]));
    executable(
        &tree.0.join("selected compiler"),
        "#!/bin/sh\necho 'relative compiler was selected' >&2\nexit 23\n",
    );
    failure(
        &tree
            .command("check")
            .env("RILL_GLSLANG_VALIDATOR", "./selected compiler")
            .output()
            .unwrap(),
        "relative compiler was selected",
    );
}

#[cfg(unix)]
#[test]
fn excessive_tool_output_is_bounded_and_cancelled() {
    let tree = Tree::new();
    fs::write(tree.0.join("shaders/mesh.vert"), "source").unwrap();
    tree.manifest(json!([{"name":"mesh","source":"shaders/mesh.vert","language":"glsl","stage":"vertex","entry":"main","target":"spirv"}]));
    let compiler = tree.0.join("noisy compiler");
    executable(
        &compiler,
        "#!/bin/sh\nhead -c 3145728 /dev/zero\nsleep 90\n",
    );
    let output = tree
        .command("check")
        .env("RILL_GLSLANG_VALIDATOR", compiler)
        .output()
        .unwrap();
    failure(&output, "tool output exceeded 2 MiB");
    assert!(output.stderr.len() < 4096);
}

#[cfg(unix)]
#[test]
fn interrupting_shader_check_kills_its_compiler_group() {
    use std::{
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    let tree = Tree::new();
    fs::write(tree.0.join("shaders/mesh.vert"), "source").unwrap();
    tree.manifest(json!([{"name":"mesh","source":"shaders/mesh.vert","language":"glsl","stage":"vertex","entry":"main","target":"spirv"}]));
    let compiler = tree.0.join("sleeping compiler");
    executable(
        &compiler,
        "#!/bin/sh\necho $$ > \"$RILL_SHADER_TEST_PID\"\nsleep 90\n",
    );
    let marker = tree.0.join("compiler.pid");
    let mut command = tree.command("check");
    command
        .env("RILL_GLSLANG_VALIDATOR", compiler)
        .env("RILL_SHADER_TEST_PID", &marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(30) {
        thread::sleep(Duration::from_millis(10));
    }
    if !marker.exists() {
        let _ = child.kill();
        let output = child.wait_with_output().unwrap();
        panic!(
            "compiler did not start: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let compiler_pid: i32 = fs::read_to_string(marker).unwrap().trim().parse().unwrap();
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let output = child.wait_with_output().unwrap();
    failure(&output, "cancelled");
    assert_ne!(
        unsafe { libc::kill(compiler_pid, 0) },
        0,
        "compiler process survived cancellation"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn concurrent_publishers_leave_one_complete_owned_generation() {
    let tree = Tree::new();
    tree.metal();
    success(&tree.command("build").output().unwrap());
    let expected = fs::read(tree.output().join("mesh.metal")).unwrap();
    let children: Vec<_> = (0..8)
        .map(|_| {
            tree.command("build")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        if !output.status.success() {
            failure(&output, "E-SHADER-BUSY");
        }
    }
    assert_eq!(
        fs::read(tree.output().join("mesh.metal")).unwrap(),
        expected
    );
    assert_eq!(metadata(&tree.output())["artifacts"][0]["name"], "mesh");
    assert_eq!(fs::read_dir(tree.output()).unwrap().count(), 3);
}

#[test]
fn artifact_names_cannot_alias_on_case_insensitive_filesystems() {
    let tree = Tree::new();
    tree.metal();
    tree.manifest(json!([
        {"name":"Mesh","source":"shaders/mesh.metal","language":"metal","stage":"vertex","entry":"vertex_main","target":"msl-source"},
        {"name":"mesh","source":"shaders/mesh.metal","language":"metal","stage":"vertex","entry":"vertex_main","target":"msl-source"}
    ]));
    failure(
        &tree.command("inspect").output().unwrap(),
        "unique without case distinctions",
    );
}

#[test]
fn real_slang_source_names_cannot_be_interpreted_as_options_or_response_files() {
    if !available("slangc", "-version") || !available("spirv-val", "--version") {
        return;
    }
    let tree = Tree::new();
    let shader = "[shader(\"vertex\")] float4 vertex_main(uint id: SV_VertexID):SV_Position { return float4(0,0,0,1); }\n";
    for source in ["-option.slang", "@response.slang"] {
        fs::write(tree.0.join(source), shader).unwrap();
    }
    tree.manifest(json!([
        {"name":"dash","source":"-option.slang","language":"slang","stage":"vertex","entry":"vertex_main","target":"spirv"},
        {"name":"response","source":"@response.slang","language":"slang","stage":"vertex","entry":"vertex_main","target":"spirv"}
    ]));
    success(&tree.command("build").output().unwrap());
    assert_eq!(
        metadata(&tree.output())["artifacts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[cfg(unix)]
#[test]
fn bundled_shader_tools_are_relocatable_and_explicit_overrides_take_precedence() {
    let tree = Tree::new();
    fs::write(tree.0.join("shaders/mesh.vert"), "source").unwrap();
    tree.manifest(json!([{"name":"mesh","source":"shaders/mesh.vert","language":"glsl","stage":"vertex","entry":"main","target":"spirv"}]));
    let bundle = tree.0.join("sdk");
    fs::create_dir(&bundle).unwrap();
    fs::create_dir(bundle.join("bin")).unwrap();
    fs::write(bundle.join("toolchain.json"), json!({"version":1,"rill":env!("CARGO_PKG_VERSION"),"tools":{"glslangValidator":"bin/compiler"},"packages":{},"runtime_dirs":[]}).to_string()).unwrap();
    executable(&bundle.join("bin/compiler"), "#!/bin/sh\nprintf '%s' \"$RILL_TOOLCHAIN_ROOT\" > \"$RILL_SHADER_TEST_ROOT\"\necho 'bundled compiler selected' >&2\nexit 23\n");
    let recorded = tree.0.join("selected-root");
    failure(
        &tree
            .command("check")
            .env("RILL_TOOLCHAIN_ROOT", "sdk")
            .env("RILL_SHADER_TEST_ROOT", &recorded)
            .output()
            .unwrap(),
        "bundled compiler selected",
    );
    assert_eq!(
        fs::read_to_string(recorded).unwrap(),
        bundle.to_str().unwrap()
    );
    let selected = tree.0.join("explicit compiler");
    executable(
        &selected,
        "#!/bin/sh\necho 'explicit compiler selected' >&2\nexit 23\n",
    );
    failure(
        &tree
            .command("check")
            .env("RILL_TOOLCHAIN_ROOT", "sdk")
            .env("RILL_GLSLANG_VALIDATOR", &selected)
            .output()
            .unwrap(),
        "explicit compiler selected",
    );
}

#[cfg(unix)]
#[test]
fn project_root_shaders_ignore_assets_and_watch_custom_published_dependencies() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    fs::write(tree.0.join("mesh.metal"), "#include <metal_stdlib>\n#include \"support.payload\"\nusing namespace metal;\nvertex float4 vertex_main() { return float4(SCALE); }\n").unwrap();
    fs::write(tree.0.join("support.payload"), "#define SCALE 1\n").unwrap();
    fs::write(tree.0.join("unrelated.asset"), vec![0; 2 * 1024 * 1024]).unwrap();
    symlink("unrelated.asset", tree.0.join("unrelated-link")).unwrap();
    fs::write(tree.0.join("main.rill"), "fn main() { 42 }").unwrap();
    fs::write(
        tree.0.join("rill.json"),
        json!({"version":2,"name":"root-shader","entry":"main.rill","shaders":"rill.shaders.json"})
            .to_string(),
    )
    .unwrap();
    tree.manifest(json!([{"name":"mesh","source":"mesh.metal","language":"metal","stage":"vertex","entry":"vertex_main","target":"msl-source"}]));
    let project = |action: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command
            .current_dir(&tree.0)
            .env_remove("RILL_TOOLCHAIN_ROOT")
            .args(["project", action]);
        command
    };
    success(&project("build").output().unwrap());
    assert!(tree.output().join("mesh.metal").exists());
    let watched = project("watch").arg("--once").output().unwrap();
    success(&watched);
    assert_eq!(watched.stdout, b"42\n");
    // A custom extension is an exact input once the owned compiler inventory
    // records it; unrelated large assets must remain ignored.
    fs::write(tree.0.join("support.payload"), vec![b' '; 2 * 1024 * 1024]).unwrap();
    failure(
        &project("watch").arg("--once").output().unwrap(),
        "E-WATCH-LIMIT",
    );
    fs::write(tree.0.join("support.payload"), "#define SCALE 2\n").unwrap();
    // Removed imports must not be held hostage by a stale dependency inventory.
    fs::write(tree.0.join("mesh.metal"), "#include <metal_stdlib>\nusing namespace metal;\nvertex float4 vertex_main() { return float4(1); }\n").unwrap();
    fs::remove_file(tree.0.join("support.payload")).unwrap();
    success(&project("build").output().unwrap());
    let mut inventory = metadata(&tree.output());
    inventory["artifacts"][0]["dependencies"][0]["path"] = "../outside-source".into();
    fs::write(tree.output().join("manifest.json"), inventory.to_string()).unwrap();
    failure(&project("build").output().unwrap(), "without traversal");
}
