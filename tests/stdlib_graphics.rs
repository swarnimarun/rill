use std::{fs, path::PathBuf, process::Command};

struct Tree(PathBuf);
impl Tree {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("rill-graphics-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn shader_metadata_is_pure_and_checked_across_backends() {
    let tree = Tree::new("metadata");
    let source = tree.0.join("main.rill");
    fs::write(&source, r#"
import std.shader as shader;
fn main() {
    let valid = record shader.Resources { samplers = 1; storage_textures = 0; storage_buffers = 2; uniform_buffers = 3 };
    let invalid = record shader.Resources { samplers = 0; storage_textures = -1; storage_buffers = 0; uniform_buffers = 0 };
    let metal = match shader.target_for_backend(true) { shader.Msl => true | shader.Spirv => false };
    let spirv = match shader.target_for_backend(false) { shader.Msl => false | shader.Spirv => true };
    shader.valid_resources(valid) && !shader.valid_resources(invalid)
        && shader.stage_code(shader.Vertex) == 0
        && shader.stage_code(shader.Fragment) == 1
        && shader.stage_code(shader.Compute) == 2
        && metal && spirv
}
"#).unwrap();
    let program = rill::modules::compile::load_program(&source).unwrap();
    for result in [
        rill::interpreter::run(&program),
        rill::interpreter::run_reference(&program),
    ] {
        assert_eq!(result.unwrap().to_string(), "true");
    }
    for (name, generated) in [
        ("typed", rill::backend::emit(&program)),
        ("reference", rill::backend::emit_reference(&program)),
    ] {
        let c = tree.0.join(format!("{name}.c"));
        let binary = tree.0.join(name);
        fs::write(&c, generated.unwrap()).unwrap();
        for opt in ["-O0", "-O3"] {
            let output = Command::new("cc")
                .args(["-std=c11", opt])
                .arg(&c)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(&binary).output().unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, b"true\n");
        }
    }
}

#[test]
fn scalar_sdl_bindings_match_real_headers_without_header_import() {
    if !Command::new("pkg-config")
        .args(["--exists", "sdl3"])
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("Skipping std.sdl ABI integration: SDL3 development files unavailable");
        return;
    }
    let tree = Tree::new("sdl");
    let source = tree.0.join("main.rill");
    fs::write(&source, "import std.sdl as sdl; fn main() { if sdl.SDL_Init(u32(0)) { sdl.SDL_Delay(u32(1)); sdl.SDL_Quit(); 42 } else { 1 } }").unwrap();
    for level in ["0", "3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("run")
            .arg(&source)
            .args(["--opt-level", level])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"42\n");
    }
}
