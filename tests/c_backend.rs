use rill::ast::{BinaryOp as Op, Expr as E, Function, Param, ParamMode, Program};
use rill::{backend, interpreter};
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn binary(op: Op, a: E, b: E) -> E {
    E::Binary(op, Box::new(a), Box::new(b))
}
fn program(body: E) -> Program {
    Program {
        native: Default::default(),
        constants: vec![],
        allocation_types: None,
        externs: vec![],
        types: vec![],
        functions: vec![Function {
            name: "main".into(),
            params: vec![],
            body,
        }],
    }
}
fn compare(p: &Program) {
    let expected = interpreter::run(p);
    let c = backend::emit(p).unwrap();
    let path = std::env::temp_dir().join(format!(
        "rill-c-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    let source = path.join("program.c");
    let executable = path.join("program");
    fs::write(&source, c).unwrap();
    for opt in ["-O0", "-O2"] {
        let build = Command::new("cc")
            .args(["-std=c11", opt])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("cc is required for backend differential tests");
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let result = Command::new(&executable).output().unwrap();
        match &expected {
            Ok(v) => {
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(String::from_utf8(result.stdout).unwrap(), format!("{v}\n"));
            }
            Err(_) => {
                assert!(!result.status.success());
                assert_eq!(result.stderr, b"rill: runtime trap\n");
            }
        }
    }
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn arithmetic_and_traps_match_c() {
    for expr in [
        binary(Op::Add, E::Int(20), E::Int(22)),
        binary(Op::Sub, E::Int(i32::MIN), E::Int(1)),
        binary(Op::Mul, E::Int(i32::MAX), E::Int(2)),
        binary(Op::Div, E::Int(1), E::Int(0)),
        binary(Op::Div, E::Int(i32::MIN), E::Int(-1)),
        binary(Op::Rem, E::Int(i32::MIN), E::Int(-1)),
        binary(Op::Div, E::Int(-7), E::Int(3)),
    ] {
        compare(&program(expr));
    }
}

#[test]
fn branches_and_short_circuit_match_c() {
    let trap = || binary(Op::Div, E::Int(1), E::Int(0));
    compare(&program(E::If(
        Box::new(E::Bool(true)),
        Box::new(E::Int(42)),
        Box::new(trap()),
    )));
    compare(&program(binary(
        Op::And,
        E::Bool(false),
        binary(Op::Eq, trap(), E::Int(1)),
    )));
    compare(&program(binary(
        Op::Or,
        E::Bool(true),
        binary(Op::Eq, trap(), E::Int(1)),
    )));
}

#[test]
fn arguments_are_emitted_in_source_order() {
    let p = Program {
        native: Default::default(),
        constants: vec![],
        allocation_types: None,
        externs: vec![],
        types: vec![],
        functions: vec![
            Function {
                name: "id".into(),
                params: vec![Param {
                    name: "x".into(),
                    mode: ParamMode::Value,
                    annotation: None,
                }],
                body: E::Var("x".into()),
            },
            Function {
                name: "add".into(),
                params: vec![
                    Param {
                        name: "a".into(),
                        mode: ParamMode::Value,
                        annotation: None,
                    },
                    Param {
                        name: "b".into(),
                        mode: ParamMode::Value,
                        annotation: None,
                    },
                ],
                body: binary(Op::Add, E::Var("a".into()), E::Var("b".into())),
            },
            Function {
                name: "main".into(),
                params: vec![],
                body: E::Call(
                    "add".into(),
                    vec![
                        E::Call("id".into(), vec![E::Int(11)]),
                        E::Call("id".into(), vec![E::Int(22)]),
                    ],
                ),
            },
        ],
    };
    let c = backend::emit(&p).unwrap();
    let first = c.find("rl_make(0, INT32_C(11))").unwrap();
    let second = c.find("rl_make(0, INT32_C(22))").unwrap();
    assert!(first < second);
    compare(&p);
}

#[test]
fn primitive_patterns_match_in_both_backends() {
    use rill::ast::{MatchArm, Pattern};
    let p = program(E::Match(
        Box::new(E::Int(9)),
        vec![
            MatchArm {
                pattern: Pattern::Int(8),
                body: E::Int(0),
            },
            MatchArm {
                pattern: Pattern::Binding("x".into()),
                body: binary(Op::Add, E::Var("x".into()), E::Int(1)),
            },
        ],
    ));
    compare(&p);
}

#[test]
fn nullary_constructors_are_inline_and_match() {
    use rill::ast::{MatchArm, Pattern, TypeDecl, VariantDecl};
    let colors = TypeDecl {
        record_fields: None,
        name: "Color".into(),
        params: vec![],
        variants: vec![
            VariantDecl {
                name: "Red".into(),
                fields: vec![],
            },
            VariantDecl {
                name: "Blue".into(),
                fields: vec![],
            },
        ],
    };
    let mut p = program(E::Match(
        Box::new(E::Var("Red".into())),
        vec![
            MatchArm {
                pattern: Pattern::Constructor("Blue".into(), vec![]),
                body: E::Int(1),
            },
            MatchArm {
                pattern: Pattern::Constructor("Red".into(), vec![]),
                body: E::Int(42),
            },
        ],
    ));
    p.types.push(colors);
    compare(&p);
    p.functions[0].body = E::Var("Blue".into());
    compare(&p);
    assert!(!backend::emit(&p).unwrap().contains("malloc("));
}

#[test]
fn payload_constructors_and_nested_patterns_match_c() {
    for source in [
        "type Pair = Pair(i32, bool)\nlet main = Pair(42, true)",
        "type Pair = Pair(i32, bool)\nlet main = match Pair(42, true) { Pair(x, true) => x | Pair(_, false) => 0 }",
        "type Option<a> = None | Some(a)\nlet main = match Some(Some(42)) { Some(Some(x)) => x | _ => 0 }",
        "type Option<a> = None | Some(a)\nlet main = Some(Some(42))",
        "type Inner = Inner(i32, bool)\ntype Outer = Outer(Inner, i32)\nlet select x = match x { Outer(Inner(n, true), _) => n | Outer(Inner(_, false), other) => other }\nlet main = select(Outer(Inner(11, false), 42))",
        "type Inner = Inner(i32, bool)\ntype Outer = Outer(Inner, i32)\nlet main = Outer(Inner(11, false), 42)",
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        compare(&program);
    }
}

#[test]
fn specialized_generic_payload_layout_matches_c() {
    let source = "type Box<a> = Box(a)\nlet id x = x\nlet main = Box(id(1))";
    let program = rill::parser::parse(source).unwrap();
    rill::check::check(&program).unwrap();
    compare(&program);
}

#[test]
fn rejects_unresolved_generic_payload_layout() {
    let source = "type Option<a> = None | Some(a)\nlet main = None";
    let program = rill::parser::parse(source).unwrap();
    rill::check::check(&program).unwrap();
    assert!(backend::emit(&program).unwrap_err().contains("E-C-ADT"));
}

#[test]
fn false_while_skips_body_in_both_backends() {
    let source = "let main = while false do let ignored = 1 / 0 in ()";
    let program = rill::parser::parse(source).unwrap();
    rill::check::check(&program).unwrap();
    compare(&program);
}

#[test]
fn specialized_intermediate_values_fit_c_inline_storage() {
    for source in [
        "type Box<a> = Box(a)\nlet id x = x\nlet main = let a = id(Box(true)) in id(Box(Box(3)))",
        "type Box<a> = Box(a)\nlet use x = let nested = Box(x) in 0\nlet main = use(Box(Box(3)))",
        "type Box<a> = Box(a)\nlet outer x = inner(Box(x))\nlet inner x = match x { Box(y) => y }\nlet main = outer(Box(Box(3)))",
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        compare(&program);
    }
}

#[test]
fn mutable_locals_loops_and_match_scopes_match_c() {
    for source in [
        "let main = let mut x = 0 in let ignored = while x < 1000 do x <- x + 1 in x",
        "let main = let mut x = 0 in let ignored = match true { true => x <- 4 | false => () } in x",
        "let main = let mut x = 3 in let ignored = let mut x = 4 in x <- 5 in x",
        "let main = let mut x = 10 in let ignored = x <- x + 1 in x",
        "type Option<a> = None | Some(a)\nlet main = let mut x = Some(1) in let ignored = x <- Some(2) in match x { Some(n) => n | None => 0 }",
        "let main = let mut x = 0 in let saved = x in let ignored = x <- 1 in saved",
        "let main = let mut x = 0 in let ignored = x <- (let inner = x <- 1 in x + 1) in x",
        "type Option<a> = None | Some(a)\nlet main = let mut x = 4 in let ignored = match Some(9) { Some(x) => () | None => () } in x",
        "type Option<a> = None | Some(a)\nlet main = let mut x = 4 in let ignored = match Some(9) { Some(y) => x <- y | None => () } in x",
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        compare(&program);
    }
}

#[test]
fn static_utf8_strings_match_c() {
    for source in [
        r#"let main = "héllo""#,
        r#"let main = "x\0y""#,
        r#"let main = byte_len("hé")"#,
        r#"let main = byte_at("é", 0)"#,
        r#"let main = byte_at("é", 1)"#,
        r#"let main = byte_at("a\0b", 1)"#,
        r#"let main = byte_at("", 0)"#,
        r#"let main = byte_at("a", -1)"#,
        r#"let main = byte_at("a", 1)"#,
        "type Box<a> = Box(a)\nlet main = Box(\"x\\0y\")",
        "type Box<a> = Box(a)\nlet main = match Box(\"hé\") { Box(s) => byte_at(s, 1) }",
        r#"let main = let mut s = "x" in let ignored = s <- "é" in byte_len(s)"#,
        r#"let main = let mut i = 0 in byte_at((let ignored = i <- 1 in "abc"), i)"#,
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        compare(&program);
    }
}

#[test]
fn fixed_inline_bytes_match_c() {
    for source in [
        "let main = bytes_len(bytes_new(0))",
        "let main = bytes_get(bytes_new(8), 7)",
        "let main = bytes_get(bytes_set(bytes_new(8), 7, 255), 7)",
        "let main = let mut b = bytes_new(8) in let before = b in let ignored = b <- bytes_set(b, 7, 12) in bytes_get(before, 7) + bytes_get(b, 7)",
        "let main = bytes_get(bytes_new(0), 0)",
        "let main = bytes_get(bytes_new(8), -1)",
        "let main = bytes_set(bytes_new(8), 8, 1)",
        "let main = bytes_set(bytes_new(8), 0, 256)",
        "let main = bytes_set(bytes_new(8), 0, -1)",
        "type Box<a> = Box(a)\nlet main = match Box(bytes_set(bytes_new(8), 4, 42)) { Box(b) => bytes_get(b, 4) }",
        "type Box<a> = Box(a)\nlet main = Box(bytes_set(bytes_new(4), 3, 255))",
        "let main = bytes_set(bytes_new(256), 255, 200)",
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        compare(&program);
    }
}

#[test]
fn recursion_depth_limit_matches_c() {
    for n in [30, 31, 1000] {
        let source = format!(
            "let recurse n = if n == 0 then 0 else recurse(n - 1)\nlet main = recurse({n})"
        );
        let program = rill::parser::parse(&source).unwrap();
        rill::check::check(&program).unwrap();
        let result = interpreter::run(&program);
        if n == 30 {
            assert_eq!(result.unwrap().to_string(), "0");
        } else {
            assert_eq!(result.unwrap_err(), "runtime trap");
        }
        compare(&program);
    }
}

#[test]
fn inout_root_loans_match_c() {
    for source in [
        "let bump (inout x) = x <- x + 1\nlet main = let mut a = 41 in let ignored = bump(inout a) in a",
        "let bump (inout x) = x <- x + 1\nlet relay (inout y) = bump(inout y)\nlet main = let mut a = 41 in let ignored = relay(inout a) in a",
        "let add value (inout x) = x <- x + value\nlet main = let mut a = 21 in let ignored = add(a, inout a) in a",
        "let update (inout x) (inout y) = let ignored = x <- x + 1 in y <- y + 2\nlet main = let mut a = 10 in let mut b = 29 in let ignored = update(inout a, inout b) in a + b",
        "let bump (inout x) = x <- x + 1\nlet main = let mut x = 7 in let ignored = let mut x = 1 in bump(inout x) in x",
        "type Box<a> = Box(a)\nlet replace (inout x) = x <- Box(42)\nlet main = let mut b = Box(0) in let ignored = replace(inout b) in match b { Box(n) => n }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        compare(&program);
    }
}

#[test]
fn shared_read_roots_match_c() {
    for source in [
        "let sum (read x) (read y) = x + y\nlet main = let a = 21 in sum(a, a)",
        "let sum (read x) (read y) = x + y\nlet relay (read z) = sum(z, z)\nlet main = let mut a = 21 in relay(a)",
        "let update (read x) (inout y) = y <- y + x\nlet main = let a = 21 in let mut b = 21 in let ignored = update(a, inout b) in b",
        "type Box<a> = Box(a)\nlet extract (read x) = match x { Box(n) => n }\nlet main = let b = Box(42) in extract(b)",
        "type Option<a> = None | Some(a)\nlet inspect (read x) = x\nlet main = match Some(42) { Some(x) => inspect(x) | None => 0 }",
        "type Option<a> = None | Some(a)\nlet inspect (read x) = x\nlet main = match Some(Some(42)) { Some(Some(x)) => inspect(x) | _ => 0 }",
        "type Option<a> = None | Some(a)\nlet inspect (read x) = x\nlet main = let mut x = 41 in let ignored = match Some(8) { Some(x) => inspect(x) | None => 0 } in let ignored2 = x <- x + 1 in x",
        "let inspect (read x) = x\nlet relay (inout x) = let observed = inspect(x) in x <- observed + 1\nlet main = let mut x = 41 in let ignored = relay(inout x) in x",
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        compare(&program);
    }
}

#[test]
fn provisional_typed_ir_matches_both_existing_paths() {
    for source in [
        "let main = 2 + 3 * 4",
        "let main = 2147483647 + 1",
        "let main = false && (1 / 0 == 0)",
        "let main = let mut x = 0 in let ignored = while x < 100 do x <- x + 1 in x",
        "let main = let mut x = 1 in let ignored = (let x = true in x) in let done = x <- x + 1 in x",
    ] {
        let program = rill::parser::parse(source).unwrap();
        let checked = rill::check::analyze(&program).unwrap();
        let ir = rill::typed::lower_main(&checked).unwrap();
        assert_eq!(rill::typed::evaluate(&ir), interpreter::run(&program), "{source}");
        compare(&program);
    }
}

#[test]
fn module_mangled_constructor_displays_source_name() {
    use rill::ast::{TypeDecl, TypeExpr, VariantDecl};
    let p = Program {
        native: Default::default(),
        constants: vec![],
        allocation_types: None,
        externs: vec![],
        types: vec![TypeDecl {
            record_fields: None,
            name: "Box".into(),
            params: vec![],
            variants: vec![VariantDecl {
                name: "M$12$Boxed".into(),
                fields: vec![TypeExpr {
                    name: "i32".into(),
                    args: vec![],
                }],
            }],
        }],
        functions: vec![Function {
            name: "main".into(),
            params: vec![],
            body: E::Call("M$12$Boxed".into(), vec![E::Int(42)]),
        }],
    };
    assert_eq!(interpreter::run(&p).unwrap().to_string(), "Boxed(42)");
    compare(&p);
}

#[test]
fn records_preserve_field_order_and_value_semantics() {
    for (source, expected) in [
        ("type Point = { x: i32; y: i32 }\nlet main = record Point { y = 2; x = 1 }", "Point { x = 1; y = 2 }"),
        ("type Point = { x: i32; y: i32 }\nlet main = let mut n = 0 in let p = record Point { y = (let ignored = n <- n + 1 in n); x = (let ignored = n <- n + 1 in n) } in p.x * 10 + p.y", "21"),
        ("type Point = { x: i32; y: i32 }\nlet main = let mut p = record Point { x = 1; y = 2 } in let old = p in let done = p.x <- (let inner = p.y <- 20 in p.y + 1) in old.x + p.x + p.y", "42"),
        ("type Option = None | Some(i32)\ntype Pair = { first: Option; second: i32 }\nlet main = let mut p = record Pair { first = None; second = 42 } in let done = p.first <- Some(7) in match p.first { Some(n) => n + p.second | None => 0 }", "49"),
        ("type Option = None | Some(i32)\ntype Pair = { first: Option; second: i32 }\nlet main = let mut p = record Pair { first = Some(7); second = 42 } in let done = p.first <- None in p.second", "42"),
        ("type Point = { x: i32; y: i32 }\nlet bump (inout p: Point) = p.x <- p.x + 1\nlet main = let mut p = record Point { x = 41; y = 0 } in let done = bump(inout p) in p.x", "42"),
    ] {
        let program = rill::parser::parse(source).unwrap();
        rill::check::check(&program).unwrap();
        assert_eq!(interpreter::run(&program).unwrap().to_string(), expected, "{source}");
        compare(&program);
    }
}

#[test]
fn field_loan_preserves_vector_payload_and_siblings() {
    let source = "type R = { v: u8x16; keep: i32 }\nlet clear (inout v: u8x16) = v <- u8x16_xor(v, v)\nlet main = let mut p = record R { v = u8x16_load(bytes_set(bytes_new(16), 0, 99), 0); keep = 42 } in let done = clear(inout p.v) in u8x16_lane(p.v, 0) + p.keep";
    let program = rill::parser::parse(source).unwrap();
    rill::check::check(&program).unwrap();
    assert_eq!(interpreter::run(&program).unwrap().to_string(), "42");
    compare(&program);
}
