#[path = "src/fingerprint.rs"]
mod fingerprint;

use std::{env, fs, path::Path, process::Command};

fn frame(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(bytes);
}

fn collect(path: &Path, inputs: &mut Vec<u8>) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(path)
            .expect("read compiler source directory")
            .map(|entry| entry.expect("read source entry").path())
            .collect();
        entries.sort();
        for entry in entries {
            collect(&entry, inputs);
        }
    } else {
        frame(
            inputs,
            path.to_str()
                .expect("UTF-8 compiler source path")
                .as_bytes(),
        );
        frame(inputs, &fs::read(path).expect("read compiler source"));
    }
}

fn main() {
    let mut inputs = b"rill-compiler-build-v1".to_vec();
    for source in ["src", "stdlib", "Cargo.toml", "Cargo.lock", "build.rs"] {
        collect(Path::new(source), &mut inputs);
    }
    let mut settings: Vec<_> = env::vars()
        .filter(|(key, _)| key.starts_with("CARGO_FEATURE_"))
        .collect();
    for name in [
        "RUSTC",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        settings.push((name.to_owned(), env::var(name).unwrap_or_default()));
    }
    settings.sort();
    for (name, value) in settings {
        frame(&mut inputs, name.as_bytes());
        frame(&mut inputs, value.as_bytes());
    }
    let rustc = Command::new(env::var_os("RUSTC").expect("Cargo RUSTC"))
        .arg("-vV")
        .output()
        .expect("query compiler identity");
    assert!(rustc.status.success(), "query compiler identity failed");
    frame(&mut inputs, &rustc.stdout);
    let id: String = fingerprint::digest(&inputs)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!("cargo:rustc-env=RILL_COMPILER_ID={id}");
}
