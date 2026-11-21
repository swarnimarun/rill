use rill::{backend, check, formatter, interpreter, parser, syntax::SyntaxTree};
use std::{fs, process::Command};

const OPTIONS: &str = "type O<a> = None | Some(a)\n";

#[test]
fn conditional_patterns_preserve_match_order_and_owner_rules_in_optimized_c() {
    let programs = [
        "fn next(inout n) { n <- n + 1; Some(Some(n)) } fn main(){ let mut n=0; let hit=if let Some(Some(x))=next(inout n){ x+40 }else{1/0}; hit+n }",
        "fn main(){ let x=42; if let Some(x)=None { 1/0+x }else{x} }",
        "fn main(){ let mut n=40; if let true = false { n <- 0; }; if let value=2 { n <- n+value; }; n }",
        "fn main(){ let b=system_box_i32_abort(42); let result=Some(move b); if let Some(x)=move result { box_read(x) }else{0} }",
        "fn main(){ if let x = 3 in x == 3 then 42 else 0 }",
        "fn main(){ if let mut x = true in x { 42 }else{0} }",
    ];
    let directory =
        std::env::temp_dir().join(format!("rill-conditional-patterns-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    for body in programs {
        let src = format!("{OPTIONS}{body}");
        let program = parser::parse(&src).unwrap();
        let formatted = formatter::format(&src).unwrap();
        assert_eq!(parser::parse(&formatted).unwrap(), program);
        SyntaxTree::parse(&src).ast_source_map().unwrap();
        assert_eq!(
            interpreter::run(&program).unwrap().to_string(),
            "42",
            "{body}"
        );
        let c = backend::emit(&program).unwrap();
        fs::write(directory.join("main.c"), c).unwrap();
        for opt in ["-O0", "-O3"] {
            let built = Command::new("cc")
                .current_dir(&directory)
                .args([
                    "-std=c11",
                    "-pedantic-errors",
                    opt,
                    "main.c",
                    "-o",
                    "program",
                ])
                .output()
                .unwrap();
            assert!(
                built.status.success(),
                "{}",
                String::from_utf8_lossy(&built.stderr)
            );
            let result = Command::new(directory.join("program")).output().unwrap();
            assert!(
                result.status.success(),
                "{body}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, b"42\n", "{body}");
        }
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn conditional_patterns_reject_missing_blocks_bad_types_and_out_of_scope_binders() {
    for body in [
        "if let Some(x) Some(1) {x}",
        "if let Some(x)=Some(1) then x else 0",
        "if let Some(x)=None {x} else 0",
        "if let Some(=Some(1){42}",
    ] {
        let source = format!("{OPTIONS}fn main(){{ {body} }}");
        assert!(parser::parse(&source).is_err());
        assert!(SyntaxTree::parse(&source).parsed_module().is_none());
    }
    for body in ["if let Some(x)=Some(1){x}", "if let Some(x)=Some(1){x}else{x}", "if let Some(x)=true{x}else{0}",
        "let b=system_box_i32_abort(42); let o=Some(move b); if let Some(x)=o {box_read(x)}else{0}",
        "let b=system_box_i32_abort(42); let o=Some(move b); if let Some(x)=move o {box_read(x)}else{0}; if let Some(x)=move o {box_read(x)}else{0}"] {
        let source = format!("{OPTIONS}fn main(){{ {body} }}");
        let p=parser::parse(&source).unwrap();
        assert!(check::analyze(&p).is_err(),"{source}");
        assert!(backend::emit(&p).is_err(),"{source}");
    }
}

#[test]
fn conditional_and_written_match_elaborate_to_the_same_program() {
    for (conditional, exhaustive) in [
        (
            "if let Some(n) = value { n } else { 42 }",
            "match value { Some(n) => { n } | _ => { 42 } }",
        ),
        (
            "if let Some(Some(n)) = value { () }",
            "match value { Some(Some(n)) => { () } | _ => () }",
        ),
        (
            "if let None = move value { 0 } else { 42 }",
            "match move value { None => { 0 } | _ => { 42 } }",
        ),
        (
            "if let n = value { n } else { 0 }",
            "match value { n => { n } | _ => { 0 } }",
        ),
    ] {
        let parse =
            |body: &str| parser::parse(&format!("{OPTIONS}fn pick(value){{ {body} }}")).unwrap();
        assert_eq!(parse(conditional), parse(exhaustive));
    }
}
