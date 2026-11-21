//! Embed the lesson corpus so an installed playground can run from any directory.
use std::{env, fs, path::Path};

fn main() {
    let directory = "examples/playground";
    println!("cargo:rerun-if-changed={directory}");
    let mut filenames = Vec::new();
    for entry in fs::read_dir(directory).expect("read playground lessons") {
        let path = entry.expect("lesson directory entry").path();
        if path.extension().is_some_and(|ext| ext == "rill") {
            filenames.push(
                path.file_name()
                    .unwrap()
                    .to_str()
                    .expect("UTF-8 lesson filename")
                    .to_owned(),
            );
        }
    }
    filenames.sort();
    let mut registry = String::from("&[\n");
    for name in filenames {
        let path = format!("/{directory}/{name}");
        registry.push_str(&format!(
            "({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {path:?}))),\n"
        ));
    }
    registry.push_str("]\n");
    let out = env::var_os("OUT_DIR").unwrap();
    fs::write(Path::new(&out).join("playground_sources.rs"), registry)
        .expect("write lesson registry");
}
