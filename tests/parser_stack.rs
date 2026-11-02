use std::process::Command;

#[test]
fn deep_input_reports_error_without_process_abort() {
    const CHILD: &str = "RILL_PARSER_STACK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "deep_input_reports_error_without_process_abort"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "child failed: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let n = 10_000;
            let inputs = [
                format!("let main = {}1{}", "(".repeat(n), ")".repeat(n)),
                format!("let main = {}1", "-".repeat(n)),
                format!("let main = 1{}", " + 1".repeat(n)),
                format!("let main = 1{} +", " + 1".repeat(n)),
                format!("let main = {}0{}", "let x = 0 in ".repeat(n), ""),
                format!("type T = C({}i32{})", "Box<".repeat(n), ">".repeat(n)),
                format!(
                    "let main = match 0 {{ {}_{} => 1 }}",
                    "C(".repeat(n),
                    ")".repeat(n)
                ),
            ];
            for source in inputs {
                let err = rill::parser::parse(&source).unwrap_err();
                assert!(err.contains("E-PARSE-DEPTH"), "{err}");
            }
            assert!(rill::parser::parse("let main = (1 + 2) * 3").is_ok());
        })
        .unwrap()
        .join()
        .unwrap();
}
