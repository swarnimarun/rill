use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: Option<&str>) {
    let program = rill::parser::parse(source).unwrap();
    compare_program(&program, source, expected);
}

fn compare_program(program: &rill::ast::Program, source: &str, expected: Option<&str>) {
    let checked = rill::check::analyze(program).unwrap_or_else(|error| panic!("{source}: {error}"));
    let typed = rill::typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
    let reference = if checked.has_owned() {
        rill::interpreter::run(program)
    } else {
        rill::interpreter::run_reference(program)
    };
    match expected {
        Some(expected) => assert_eq!(
            reference.as_ref().unwrap().to_string(),
            expected,
            "{source}"
        ),
        None => assert!(reference.is_err(), "{source}"),
    }
    assert_eq!(rill::typed::evaluate(&typed), reference, "{source}");
    assert_eq!(rill::interpreter::run(program), reference, "{source}");
    let c = rill::typed::emit_c(&typed).unwrap();
    assert_eq!(rill::backend::emit(program).unwrap(), c, "{source}");
    assert!(!c.contains("rl_value") && !c.contains("RL_CAP"), "{source}");
    assert!(!c.contains("restrict"), "{source}");
    let dir = std::env::temp_dir().join(format!(
        "rill-typed-vector-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let source_path = dir.join("main.c");
    let binary = dir.join("main");
    let mut emitted = vec![c];
    if !checked.has_owned() {
        // Force the old native representation without pretending raw-C values
        // have interpreter semantics. The unused marker changes no result.
        let fallback_source =
            source.replacen("fn main() {", "fn main() { let native_marker = u32(0);", 1);
        let fallback_program = rill::parser::parse(&fallback_source).unwrap();
        let fallback = rill::backend::emit(&fallback_program).unwrap();
        assert!(fallback.contains("rl_value"));
        emitted.push(fallback);
    }
    for c in emitted {
        fs::write(&source_path, c).unwrap();
        for opt in ["-O0", "-O2"] {
            let output = Command::new("cc")
                .args(["-std=c11", "-pedantic-errors", opt])
                .arg(&source_path)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(&binary).output().unwrap();
            match expected {
                Some(expected) => {
                    assert!(
                        output.status.success(),
                        "{source}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    assert_eq!(
                        output.stdout,
                        format!("{expected}\n").as_bytes(),
                        "{source}"
                    );
                    assert!(output.stderr.is_empty());
                }
                None => {
                    assert!(!output.status.success(), "{source}");
                    assert_eq!(output.stderr, b"rill: runtime trap\n", "{source}");
                }
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vector_values_share_nominal_generic_record_and_loan_execution() {
    for (source,expected) in [
        ("fn main() { u8x16_load(bytes_new(16), 0) }","u8x16[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]"),
        ("fn main() { u8x16_lane(u8x16_load(bytes_set(bytes_new(17), 16, 42), 1), 15) }","42"),
        ("fn main() { u8x16_lane(u8x16_load(bytes_set(bytes_new(256), 255, 42), 240), 15) }","42"),
        ("fn main() { u8x16_all(u8x16_eq(u8x16_load(bytes_new(16), 0), u8x16_load(bytes_new(16), 0))) }","true"),
        ("fn main() { u8x16_any(u8x16_load(bytes_new(16), 0)) }","false"),
        ("fn main() { u8x16_all(u8x16_load(bytes_set(bytes_new(16), 0, 255), 0)) }","false"),
        ("type R = { vector: u8x16; other: i32 }\nfn replace(inout x, value) { x <- value }\nfn copy(read x) { x }\nfn main() { let mut r = record R { vector = u8x16_load(bytes_new(16), 0); other = 1 }; let old = copy(r.vector); replace(inout r.vector, u8x16_load(bytes_set(bytes_new(16), 0, 42), 0)); u8x16_lane(old, 0) + u8x16_lane(r.vector, 0) }","42"),
        ("type R = { a: u8x16; b: u8x16 }\nfn swap(inout a, inout b) { let old = a; a <- b; b <- old }\nfn forward(inout r: R) { swap(inout r.a, inout r.b) }\nfn main() { let mut r = record R { a = u8x16_load(bytes_set(bytes_new(16), 0, 40), 0); b = u8x16_load(bytes_set(bytes_new(16), 0, 2), 0) }; forward(inout r); u8x16_lane(r.a, 0) + u8x16_lane(r.b, 0) }","42"),
        ("type E<a> = A(a) | B\nfn id(x) { x }\nfn get(read v) { u8x16_lane(v, 0) }\nfn main() { let e = id(A(u8x16_load(bytes_set(bytes_new(16), 0, 42), 0))); match e { A(v) => get(v) | B => 0 } }","42"),
        ("fn main() { let mut v = u8x16_load(bytes_set(bytes_new(16), 0, 17), 0); let out = u8x16_xor(v, { v <- u8x16_load(bytes_set(bytes_new(16), 0, 93), 0); v }); u8x16_lane(out, 0) }","76"),
        ("fn main() { let mut b = bytes_new(17); let v = u8x16_load(b, { b <- bytes_set(b, 0, 42); 0 }); u8x16_lane(v, 0) }","0"),
        ("fn main() { let mut v = u8x16_load(bytes_set(bytes_new(16), 0, 42), 0); u8x16_lane(v, { v <- u8x16_load(bytes_new(16), 0); 0 }) }","42"),
        ("type R = { vector: u8x16 }\nfn main() { let r = record R { vector = u8x16_load(bytes_new(16), 0) }; match r.vector { v => u8x16_any(v) } }","false"),
        ("fn recurse(n, v) { if n == 0 { u8x16_lane(v, 0) } else { recurse(n - 1, v) } }\nfn main() { recurse(30, u8x16_load(bytes_set(bytes_new(16), 0, 42), 0)) }","42"),
    ] {compare(source,Some(expected));}
}

#[test]
fn all_vector_lanes_match_independent_scalar_results() {
    let mut seed = 0x12345678u32;
    for _ in 0..4 {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        for lane in a.iter_mut().chain(b.iter_mut()) {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *lane = (seed >> 24) as u8;
        }
        // Keep the independent uniform backend below its existing conservative
        // per-function frame limit while exercising every generated lane.
        let mut functions = String::new();
        for (name, lanes) in [("a", a), ("b", b)] {
            functions.push_str(&format!(
                "fn make_{name}() {{ let mut bytes = bytes_new(16); "
            ));
            for (i, lane) in lanes.iter().enumerate() {
                functions.push_str(&format!("bytes <- bytes_set(bytes, {i}, {lane}); "));
            }
            functions.push_str("bytes }\n");
        }
        let prefix = "let x = u8x16_load(make_a(), 0); let y = u8x16_load(make_b(), 0);";
        for op in ["and", "or", "xor", "eq"] {
            let lanes = (0..16)
                .map(|i| match op {
                    "and" => a[i] & b[i],
                    "or" => a[i] | b[i],
                    "xor" => a[i] ^ b[i],
                    "eq" => {
                        if a[i] == b[i] {
                            255
                        } else {
                            0
                        }
                    }
                    _ => unreachable!(),
                })
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            compare(
                &format!("{functions}fn main() {{ {prefix} u8x16_{op}(x, y) }}"),
                Some(&format!("u8x16[{lanes}]")),
            );
        }
    }
}

#[test]
fn vector_bounds_and_operand_traps_remain_ordered() {
    for expr in [
        "u8x16_load(bytes_new(0), 0)",
        "u8x16_load(bytes_new(15), 0)",
        "u8x16_load(bytes_new(16), -1)",
        "u8x16_load(bytes_new(16), 1)",
        "u8x16_load(bytes_new(256), 241)",
        "u8x16_load(bytes_new(16), 2147483647)",
        "u8x16_lane(u8x16_load(bytes_new(16), 0), -1)",
        "u8x16_lane(u8x16_load(bytes_new(16), 0), 16)",
        "u8x16_lane(u8x16_load(bytes_new(16), 0), -2147483647 - 1)",
        "u8x16_load({ let n = 1 / 0; bytes_new(16) }, 0)",
        "u8x16_xor(u8x16_load(bytes_new(16), 0), { let n = 1 / 0; u8x16_load(bytes_new(16), 0) })",
    ] {
        compare(&format!("fn main() {{ {expr} }}"), None);
    }
    compare("fn recurse(n, v) { if n == 0 { u8x16_lane(v, 0) } else { recurse(n - 1, v) } }\nfn main() { recurse(31, u8x16_load(bytes_new(16), 0)) }",None);
}

#[test]
fn copy_vectors_coexist_with_restricted_owner_effects_and_cleanup() {
    for source in [
        "fn main() { let owner = system_box_i32_abort(42); let v = u8x16_load(bytes_set(bytes_new(16), 0, box_read(owner)), 0); u8x16_lane(v, 0) }",
        "fn main() { let v = u8x16_load({ let owner = system_box_i32_abort(42); bytes_set(bytes_new(16), 0, box_read(owner)) }, 0); u8x16_lane(v, 0) }",
        "fn main() { let v = u8x16_xor(u8x16_load({ let a = system_box_i32_abort(40); bytes_set(bytes_new(16), 0, box_read(a)) }, 0), u8x16_load({ let b = system_box_i32_abort(2); bytes_set(bytes_new(16), 0, box_read(b)) }, 0)); u8x16_lane(v, 0) }",
        "type R = { vector: u8x16 }\nfn replace(inout v: u8x16, owner: SystemBoxI32) { v <- u8x16_load(bytes_set(bytes_new(16), 0, box_read(owner)), 0) }\nfn main() { let owner = system_box_i32_abort(42); let mut r = record R { vector = u8x16_load(bytes_new(16), 0) }; replace(inout r.vector, move owner); u8x16_lane(r.vector, 0) }",
        "type E = Payload(u8x16, SystemBoxI32)\nfn main() { let owner = system_box_i32_abort(40); let e = Payload(u8x16_load(bytes_set(bytes_new(16), 0, 2), 0), move owner); match move e { Payload(v, b) => u8x16_lane(v, 0) + box_read(b) } }",
    ]{compare(source,Some("42"));}
    for source in [
        "fn main() { let owner = system_box_i32_abort(42); u8x16_load(bytes_set(bytes_new(16), 0, box_read(owner)), 0) }",
        "fn make(owner: SystemBoxI32) { u8x16_load(bytes_set(bytes_new(16), 0, box_read(owner)), 0) }\nfn main() { let owner = system_box_i32_abort(42); make(move owner) }",
    ] {compare(source,Some("u8x16[42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]"));}
}

#[test]
fn vector_layout_and_nested_display_remain_compact() {
    let source="type E = V(u8x16)\ntype R = { vector: E }\nfn main() { record R { vector = V(u8x16_load(bytes_new(16), 0)) } }";
    compare(
        source,
        Some("R { vector = V(u8x16[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]) }"),
    );
    let p = rill::parser::parse(source).unwrap();
    let c = rill::backend::emit(&p).unwrap();
    assert!(c.contains("_Static_assert(sizeof(rl_u8x16) == 16"));
    assert!(!c.contains("RL_CAP") && !c.contains("rl_value"));
}

#[test]
fn vector_source_types_loans_and_real_owner_restrictions_remain_checked() {
    for source in [
        "fn main() { u8x16_load(bytes_new(16)) }",
        "fn main() { u8x16_load(42, 0) }",
        "fn main() { u8x16_load(bytes_new(16), true) }",
        "fn main() { u8x16_any(bytes_new(16)) }",
        "fn main() { u8x16_lane(u8x16_load(bytes_new(16), 0), false) }",
        "fn main() { u8x16_xor(u8x16_load(bytes_new(16), 0), bytes_new(16)) }",
        "fn main() { u8x16_load(bytes_new(16), 0) + u8x16_load(bytes_new(16), 0) }",
        "fn get(read v) { u8x16_any(v) }\nfn change(inout v) { v <- u8x16_load(bytes_new(16), 0) }\nfn bad(read v) { change(inout v) }\nfn main() { bad(u8x16_load(bytes_new(16), 0)) }",
        "type R = { v: u8x16 }\nfn set(inout v, n) { v <- u8x16_load(bytes_new(16), 0) }\nfn main() { let mut r = record R { v = u8x16_load(bytes_new(16), 0) }; set(inout r.v, u8x16_lane(r.v, 0)) }",
        "fn main() { let owner = system_box_i32_abort(42); u8x16_load({ let moved = move owner; bytes_new(16) }, { let n = box_read(owner); 0 }) }",
        "fn id(x) { x }\nfn main() { let owner = system_box_i32_abort(42); let v = u8x16_load(bytes_new(16), 0); box_read(owner) }",
        "fn recur(n, v) { if n == 0 { v } else { recur(n - 1, v) } }\nfn main() { let owner = system_box_i32_abort(42); let v = recur(1, u8x16_load(bytes_new(16), 0)); box_read(owner) }",
        "type R = { owner: SystemBoxI32; vector: u8x16 }\nfn main() { let owner = system_box_i32_abort(42); let r = record R { owner = move owner; vector = u8x16_load(bytes_new(16), 0) }; 42 }",
    ] {
        let p=rill::parser::parse(source).unwrap();
        assert!(rill::check::analyze(&p).is_err(),"accepted: {source}");
        assert!(rill::interpreter::run(&p).is_err());assert!(rill::backend::emit(&p).is_err());
    }
}
