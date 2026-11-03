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

#[test]
fn nested_copy_scopes_preserve_the_full_supported_call_depth() {
    const CHILD: &str = "RILL_COPY_SCOPE_GUARD_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "nested_copy_scopes_preserve_the_full_supported_call_depth",
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
    let locals = (0..8)
        .map(|i| format!("let x{i} = 0; "))
        .collect::<String>();
    let source = format!(
        "fn f(n) {{ {locals}if n == 0 {{ 42 }} else {{ f(n - 1) }} }}\nfn main() {{ f(30) }}"
    );
    let program = rill::parser::parse(&source).unwrap();
    let checked = rill::check::analyze(&program).unwrap();
    let typed = rill::typed::lower_main(&checked).unwrap();
    let expected = rill::interpreter::run_reference(&program).unwrap();
    assert_eq!(expected.to_string(), "42");
    assert_eq!(rill::typed::evaluate(&typed).unwrap(), expected);
    assert_eq!(rill::interpreter::run(&program).unwrap(), expected);
}

#[test]
fn long_copy_blocks_do_not_multiply_recursive_host_frames() {
    const CHILD: &str = "RILL_LONG_COPY_BLOCK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "long_copy_blocks_do_not_multiply_recursive_host_frames",
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
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for bindings in [16, 24] {
                let locals = (0..bindings)
                    .map(|i| format!("let x{i} = 0; "))
                    .collect::<String>();
                for calls in [30, 31] {
                    let source = format!(
                        "fn f(n, x) {{ {locals}if n == 0 {{ x }} else {{ f(n - 1, x) }} }}\nfn main() {{ f({calls}, 42) }}"
                    );
                    let program = rill::parser::parse(&source).unwrap();
                    let checked = rill::check::analyze(&program).unwrap();
                    let typed = rill::typed::lower_main(&checked).unwrap();
                    let expected = if calls == 30 {
                        Ok(rill::interpreter::Value::Int(42))
                    } else {
                        Err("runtime trap".to_owned())
                    };
                    assert_eq!(rill::typed::evaluate(&typed), expected);
                    assert_eq!(rill::interpreter::run(&program), expected);
                }
                for functions in [31, 32] {
                    let mut source = String::new();
                    for index in 0..functions {
                        let tail = if index + 1 == functions {
                            "move owner".to_owned()
                        } else {
                            format!("f{}(move owner)", index + 1)
                        };
                        source.push_str(&format!(
                            "fn f{index}(owner) {{ let held = box_read(owner); {locals}{tail} }}\n"
                        ));
                    }
                    source.push_str("fn main() { let owner = system_box_i32_abort(42); let returned = f0(move owner); box_read(returned) }");
                    let program = rill::parser::parse(&source).unwrap();
                    let checked = rill::check::analyze(&program).unwrap();
                    let typed = rill::typed::lower_main(&checked).unwrap();
                    let expected = if functions == 31 {
                        Ok(rill::interpreter::Value::Int(42))
                    } else {
                        Err("runtime trap".to_owned())
                    };
                    assert_eq!(rill::typed::evaluate(&typed), expected);
                    assert_eq!(rill::interpreter::run(&program), expected);
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
