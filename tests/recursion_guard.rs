use std::process::Command;

#[test]
fn nested_json_traps_before_interpreter_stack_overflow() {
    const CHILD: &str = "RILL_RECURSION_GUARD_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "nested_json_traps_before_interpreter_stack_overflow",
            ])
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
    let base = include_str!("../examples/json/validator.rill");
    let (functions, _) = base.rsplit_once("\nlet main =").unwrap();
    let nested = format!("{}0{}", "[".repeat(64), "]".repeat(64));
    let program = format!(
        "{}\nlet main = json_valid(\"{nested}\")\n",
        functions.replace("depth >= 8", "depth >= 64")
    );
    let program = rill::parser::parse(&program).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap_err(),
        "runtime trap"
    );
}

#[test]
fn owner_call_chain_traps_before_interpreter_stack_overflow() {
    const CHILD: &str = "RILL_OWNER_CALL_GUARD_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "owner_call_chain_traps_before_interpreter_stack_overflow",
            ])
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
    let mut source = String::new();
    for i in 0..31 {
        source.push_str(&format!("let f{i} x = f{}(x)\n", i + 1));
    }
    source.push_str(
        "let f31 x = x + 0\nlet main = let b = system_box_i32_abort(1) in f0(box_read(b))\n",
    );
    let program = rill::parser::parse(&source).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap_err(),
        "runtime trap"
    );
}
