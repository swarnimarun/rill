#[cfg(any(not(feature = "c-import"), not(unix)))]
#[test]
fn unavailable_importer_fails_explicitly_without_publishing() {
    use std::{fs, process::Command};
    let directory =
        std::env::temp_dir().join(format!("rill-import-availability-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let destination = directory.join("bindings.rill");
    fs::write(&destination, "sentinel\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .args(["import-c", "--header", "api.h", "--function", "selected"])
        .arg("-o")
        .arg(&destination)
        .env("LIBCLANG_PATH", directory.join("no libclang here"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    #[cfg(not(feature = "c-import"))]
    assert!(
        stderr.contains("requires a build with --features c-import"),
        "{stderr}"
    );
    #[cfg(all(feature = "c-import", not(unix)))]
    assert!(stderr.contains("supported only on Unix"), "{stderr}");
    assert_eq!(fs::read_to_string(&destination).unwrap(), "sentinel\n");
    fs::remove_dir_all(directory).unwrap();
}
