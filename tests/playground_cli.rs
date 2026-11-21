use serde_json::Value;
use std::process::Command;

#[test]
fn standalone_binary_replays_the_entire_embedded_curriculum() {
    let lessons: Vec<Value> =
        serde_json::from_str(include_str!("../examples/playground/lessons.json")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rill"))
        .current_dir(std::env::temp_dir())
        .args(["playground", "--check-lessons"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let results = String::from_utf8(output.stdout).unwrap();
    assert_eq!(results.lines().count(), lessons.len());
    for lesson in &lessons {
        assert!(results
            .lines()
            .any(|line| line == format!("ok  {}", lesson["id"].as_str().unwrap())));
    }
    assert!(lessons.iter().any(|lesson| lesson["ok"] == false));
}

#[test]
fn playground_arguments_and_usage_are_available_without_a_socket() {
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rill"))
            .args(args)
            .output()
            .unwrap()
    };
    let help = invoke(&["playground", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--check-lessons"));
    for args in [
        &["playground", "--port", "65536"][..],
        &["playground", "--port"],
        &["playground", "--check-lessons", "extra"],
    ] {
        assert!(!invoke(args).status.success());
    }
}
