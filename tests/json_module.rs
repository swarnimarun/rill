use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn exported_json_validator_works_through_file_modules() {
    let standalone = include_str!("../examples/json/validator.rill");
    let (body, _) = standalone.rsplit_once("\nlet main =").unwrap();
    let expected = body.replacen("let json_valid text =", "export let json_valid text =", 1);
    let library = include_str!("../examples/json/modules/validator.rill");
    assert_eq!(
        library.trim_end(),
        expected.trim_end(),
        "keep the module and standalone validator in sync"
    );
    let dir = std::env::temp_dir().join(format!(
        "rill-json-module-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("validator.rill"), library).unwrap();
    let entry = dir.join("main.rill");
    let binary = dir.join("json");
    for (literal, expected) in [("[1,true,null]", "true"), ("[1,]", "false")] {
        fs::write(
            &entry,
            format!("import validator; let main = validator.json_valid(\"{literal}\")"),
        )
        .unwrap();
        let p = rill::modules::compile::load_program(&entry).unwrap();
        assert_eq!(rill::interpreter::run(&p).unwrap().to_string(), expected);
        let run = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("run")
            .arg(&entry)
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
        let build = Command::new(env!("CARGO_BIN_EXE_rill"))
            .arg("build")
            .arg(&entry)
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
