// Mirrors the two explicit reduction traces and the typing premises in
// proofs/OptionCore.lean. The compiler has no dependency on Lean.
use rill::{backend, check, interpreter, parser};
use std::{fs, process::Command};

const DECL: &str = "type Option<a> = None | Some(a)";

#[test]
fn erased_option_core_traces_reach_the_same_integer() {
    let dir = std::env::temp_dir().join(format!("rill-option-proof-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for (cond, expected) in [(true, "42"), (false, "7")] {
        let text = format!(
            "{DECL}\nfn main() -> i32 {{ match (if {cond} {{ Some(42) }} else {{ None }}) \
             {{ Some(x) => x | None => 7 }} }}"
        );
        let program = parser::parse(&text).unwrap();
        check::check(&program).unwrap();
        assert_eq!(interpreter::run(&program).unwrap().to_string(), expected);
        assert_eq!(
            interpreter::run_reference(&program).unwrap().to_string(),
            expected
        );
        fs::write(dir.join("main.rill"), text).unwrap();
        for opt in ["0", "3"] {
            let run = Command::new(env!("CARGO_BIN_EXE_rill"))
                .current_dir(&dir)
                .args(["run", "main.rill", "--opt-level", opt])
                .output()
                .unwrap();
            assert!(
                run.status.success(),
                "{}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(String::from_utf8(run.stdout).unwrap().trim_end(), expected);
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn incompatible_option_elimination_and_branch_constraints_reject() {
    for body in [
        "match Some(false) { Some(x) => x | None => 7 }",
        "match 42 { Some(x) => x | None => 7 }",
        "if None { 42 } else { 7 }",
        "if true { Some(42) } else { false }",
    ] {
        let p = parser::parse(&format!("{DECL}\nfn main() {{ {body} }}")).unwrap();
        let error = check::check(&p).unwrap_err();
        assert!(error.starts_with("E-TYPE:"), "{body}: {error}");
        assert_eq!(interpreter::run(&p).unwrap_err(), error);
        assert_eq!(backend::emit(&p).unwrap_err(), error);
    }
}
