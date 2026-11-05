use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compare(source: &str, expected: &str) {
    let program = rill::parser::parse(source).unwrap();
    let checked = rill::check::analyze(&program).unwrap_or_else(|e| panic!("{source}: {e}"));
    let typed = rill::typed::lower_main(&checked).unwrap_or_else(|e| panic!("{source}: {e}"));
    for value in [
        rill::interpreter::run_reference(&program),
        rill::typed::evaluate(&typed),
        rill::interpreter::run(&program),
    ] {
        assert_eq!(value.unwrap().to_string(), expected, "{source}");
    }
    let dir = std::env::temp_dir().join(format!(
        "rill-f32-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("main.c");
    let binary = dir.join("main");
    for c in [
        rill::typed::emit_c(&typed).unwrap(),
        rill::backend::emit_reference(&program).unwrap(),
    ] {
        fs::write(&path, c).unwrap();
        for opt in ["-O0", "-O3"] {
            let output = Command::new("cc")
                .args(["-std=c11", "-pedantic-errors", opt])
                .arg(&path)
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
            assert!(
                output.status.success(),
                "{source}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("{expected}\n"),
                "{source} {opt}"
            );
            assert!(output.stderr.is_empty());
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn arithmetic_and_ieee_values_agree_across_all_paths() {
    for (expression, expected) in [
        ("f32_add(f32(40), f32(2))", "42"),
        ("f32_sub(f32(44), f32(2))", "42"),
        ("f32_mul(f32(21), f32(2))", "42"),
        ("f32_div(f32(1), f32(10))", "0.100000001"),
        ("f32_neg(f32(0))", "-0"),
        ("f32_div(f32(1), f32(0))", "inf"),
        ("f32_div(f32(-1), f32(0))", "-inf"),
        ("f32_div(f32(0), f32(0))", "nan"),
        ("f32(16777217)", "16777216"),
        ("f32(2147483647)", "2.14748365e+09"),
        ("f32_div(f32(1), f32(10000))", "9.99999975e-05"),
        ("f32_div(f32(1), f32(1000))", "0.00100000005"),
        ("f32_mul(f32_div(f32(1), f32(1000000000)), f32_div(f32(1), f32(1000000000)))", "9.99999942e-19"),
        ("f32_is_finite(f32(1))", "true"),
        ("f32_is_finite(f32_div(f32(0), f32(0)))", "false"),
        ("f32_is_finite(f32_div(f32(1), f32(0)))", "false"),
        ("f32_lt(f32(1), f32(2)) && f32_le(f32(2), f32(2)) && f32_gt(f32(3), f32(2)) && f32_ge(f32(3), f32(3))", "true"),
        ("f32_neg(f32(0)) == f32(0)", "true"),
        ("f32_div(f32(0), f32(0)) == f32_div(f32(0), f32(0))", "false"),
        ("f32_div(f32(0), f32(0)) != f32_div(f32(0), f32(0))", "true"),
        ("f32_le(f32_div(f32(0), f32(0)), f32(1))", "false"),
        ("f32_add(f32_mul(f32(4097), f32(4097)), f32(-16785408))", "0"),
    ] { compare(&format!("fn main() {{ {expression} }}"), expected); }
}

#[test]
fn copy_products_generics_and_operand_snapshots_preserve_float_values() {
    compare("type Vec3 = { x: f32; y: f32; z: f32 } fn dot(a: Vec3, b: Vec3) { f32_add(f32_add(f32_mul(a.x, b.x), f32_mul(a.y, b.y)), f32_mul(a.z, b.z)) } fn main() { let a = record Vec3 { x = f32(1); y = f32(2); z = f32(3) }; dot(a, a) }", "14");
    compare("type Wrap<a> = Wrap(a) fn id(x) { x } fn main() { match id(Wrap(f32_div(f32(1), f32(10)))) { Wrap(x) => x } }", "0.100000001");
    compare("fn replace(inout value: f32) { value <- f32(2); f32(3) } fn main() { let mut value = f32(40); f32_add(value, replace(inout value)) }", "43");
}

#[test]
fn invalid_float_calls_and_implicit_operators_remain_diagnostics() {
    for expression in [
        "f32_add(f32(1))",
        "f32_neg(f32(1), f32(2))",
        "f32_is_finite(1)",
        "f32_mul(f32(1), 2)",
        "f32_div(true, false)",
        "f32(1) + f32(2)",
        "f32(1) < f32(2)",
        "f32(1.5)",
    ] {
        assert!(
            rill::compile(&format!("fn main() {{ {expression} }}")).is_err(),
            "{expression}"
        );
    }
    let program = rill::parser::parse("fn main() { f32(1 / 0) }").unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap_err(),
        "runtime trap"
    );
    assert_eq!(
        rill::interpreter::run_reference(&program).unwrap_err(),
        "runtime trap"
    );
}

#[test]
fn unsafe_float_flags_are_rejected_only_for_float_programs() {
    let dir = std::env::temp_dir().join(format!("rill-f32-flags-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    for source in [
        "fn main() { f32(1) }",
        "fn main() { f32_add(f32(1), f32(2)) }",
        "fn main() { 42 }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        for c in [
            rill::backend::emit(&program).unwrap(),
            rill::backend::emit_reference(&program).unwrap(),
        ] {
            let path = dir.join("main.c");
            fs::write(&path, c).unwrap();
            for flag in ["-ffast-math", "-ffinite-math-only"] {
                let output = Command::new("cc")
                    .args(["-std=c11", flag, "-c"])
                    .arg(&path)
                    .arg("-o")
                    .arg(dir.join("main.o"))
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.success(),
                    !source.contains("f32"),
                    "{source} {flag}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
    fs::remove_dir_all(dir).unwrap();
}
