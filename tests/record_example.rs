use std::{fs, process::Command};

#[test]
fn damage_example_handles_shield_spillover_and_lethal_hit() {
    let source = include_str!("../examples/records/damage.rill");
    let dir = std::env::temp_dir().join(format!("rill-record-example-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let cfile = dir.join("program.c");
    let binary = dir.join("program");
    assert!(source.contains("inout status.shield, 45)"));
    let original = rill::parser::parse(source).unwrap();
    assert_eq!(rill::interpreter::run(&original).unwrap().to_string(), "85");
    assert!(source.contains("\n    status.health\n"));
    for (damage, expected) in [(20, 10010), (45, 8500), (150, 0)] {
        let program = source
            .replace(
                "inout status.shield, 45)",
                &format!("inout status.shield, {damage})"),
            )
            .replace(
                "\n    status.health\n",
                "\n    status.health * 100 + status.shield\n",
            );
        let parsed = rill::parser::parse(&program).unwrap();
        rill::check::check(&parsed).unwrap();
        assert_eq!(
            rill::interpreter::run(&parsed).unwrap().to_string(),
            expected.to_string()
        );
        fs::write(&cfile, rill::compile(&program).unwrap()).unwrap();
        let build = Command::new("cc")
            .args(["-std=c11", "-O2"])
            .arg(&cfile)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let result = Command::new(&binary).output().unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, format!("{expected}\n").as_bytes());
    }
    fs::remove_dir_all(dir).unwrap();
}
