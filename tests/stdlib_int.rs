use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill-stdlib-int-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compare(source: &str, reference: bool) {
    let fixture = Fixture::new();
    let entry = fixture.0.join("main.rill");
    fs::write(&entry, source).unwrap();
    let program = rill::modules::compile::load_program(&entry).unwrap();
    assert_eq!(
        rill::interpreter::run(&program).unwrap().to_string(),
        "true"
    );
    if reference {
        assert_eq!(
            rill::interpreter::run_reference(&program)
                .unwrap()
                .to_string(),
            "true"
        );
    }
    let c = fixture.0.join("main.c");
    fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
    let executable = fixture.0.join("main");
    for optimization in ["-O0", "-O3"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&c)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&executable).output().unwrap();
        assert!(
            run.status.success(),
            "{optimization}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"true\n", "{optimization}");
        assert!(run.stderr.is_empty(), "{optimization}");
    }
}

fn integer(value: i32) -> String {
    if value == i32::MIN {
        "(-2147483647 - 1)".into()
    } else {
        value.to_string()
    }
}

fn result(value: i64) -> (i32, i32) {
    match i32::try_from(value) {
        Ok(value) => (0, value),
        Err(_) => (1, 0),
    }
}

fn check(operation: &str, expected: (i32, i32)) -> String {
    format!(
        "expect({operation}, {}, {})",
        expected.0,
        integer(expected.1)
    )
}

fn source(cases: &[String]) -> String {
    let mut source = String::from(
        "import std.int as integer; import std.result as result;\n\
         fn expect(actual: result.Result<i32, integer.Error>, code: i32, expected: i32) -> bool {\n\
           match actual { result.Ok(value) => code == 0 && value == expected\n\
           | result.Err(error) => match error { integer.Overflow => code == 1\n\
             | integer.DivisionByZero => code == 2 | integer.InvalidBounds => code == 3 } }\n\
         }\n",
    );
    for (index, body) in cases.iter().enumerate() {
        source.push_str(&format!("fn case{index}() -> bool {{ {body} }}\n"));
    }
    // Small groups keep source nesting and active call depth independent of
    // the number of oracle cases.
    let groups = cases.len().div_ceil(16);
    for group in 0..groups {
        let body = (group * 16..((group + 1) * 16).min(cases.len()))
            .map(|index| format!("case{index}()"))
            .collect::<Vec<_>>()
            .join(" && ");
        source.push_str(&format!("fn group{group}() -> bool {{ {body} }}\n"));
    }
    source.push_str("fn main() -> bool { ");
    source.push_str(
        &(0..groups)
            .map(|group| format!("group{group}()"))
            .collect::<Vec<_>>()
            .join(" && "),
    );
    source.push_str(" }\n");
    source
}

#[test]
fn binary_arithmetic_matches_wide_oracle_at_boundaries_and_sampled_values() {
    let boundaries = [
        i32::MIN,
        i32::MIN + 1,
        -1_073_741_824,
        -1_073_741_823,
        -46_341,
        -46_340,
        -2,
        -1,
        0,
        1,
        2,
        46_340,
        46_341,
        1_073_741_823,
        1_073_741_824,
        i32::MAX - 1,
        i32::MAX,
    ];
    let mut pairs: Vec<_> = boundaries
        .into_iter()
        .flat_map(|left| boundaries.into_iter().map(move |right| (left, right)))
        .collect();
    // A fixed sequence adds unrelated bit patterns without nondeterminism.
    let mut random = 0x91E1_0DA5_u32;
    for _ in 0..64 {
        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let left = random as i32;
        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        pairs.push((left, random as i32));
    }
    let mut cases = Vec::new();
    for (left, right) in pairs {
        let a = i64::from(left);
        let b = i64::from(right);
        let arguments = format!("{}, {}", integer(left), integer(right));
        let division = if right == 0 { (2, 0) } else { result(a / b) };
        let remainder = if right == 0 {
            (2, 0)
        } else if left == i32::MIN && right == -1 {
            // Rill's primitive remainder shares division's overflow trap.
            (1, 0)
        } else {
            result(a % b)
        };
        let mut checks = [
            ("checked_add", result(a + b)),
            ("checked_sub", result(a - b)),
            ("checked_mul", result(a * b)),
            ("checked_div", division),
            ("checked_rem", remainder),
        ]
        .map(|(name, expected)| check(&format!("integer.{name}({arguments})"), expected))
        .to_vec();
        for (name, expected) in [
            ("saturating_add", a + b),
            ("saturating_sub", a - b),
            ("saturating_mul", a * b),
        ] {
            let expected = expected.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            checks.push(format!(
                "integer.{name}({arguments}) == {}",
                integer(expected)
            ));
        }
        checks.push(format!(
            "integer.min({arguments}) == {} && integer.max({arguments}) == {}",
            integer(left.min(right)),
            integer(left.max(right))
        ));
        cases.push(checks.join(" && "));
    }
    compare(&source(&cases), true);
}

#[test]
fn unary_limits_and_clamp_return_exact_results_and_errors() {
    let values = [i32::MIN, i32::MIN + 1, -42, -1, 0, 1, 42, i32::MAX];
    let mut cases = vec![
        "integer.min_value() == (-2147483647 - 1) && integer.max_value() == 2147483647".into(),
    ];
    for value in values {
        let argument = integer(value);
        cases.push(format!(
            "{} && {} && integer.signum({argument}) == {}",
            check(
                &format!("integer.checked_neg({argument})"),
                result(-i64::from(value))
            ),
            check(
                &format!("integer.checked_abs({argument})"),
                result(i64::from(value).abs())
            ),
            value.signum()
        ));
        for (lower, upper) in [
            (i32::MIN, i32::MAX),
            (-10, 10),
            (0, 0),
            (i32::MAX, i32::MAX),
            (i32::MIN, i32::MIN),
            (1, -1),
            (i32::MAX, i32::MIN),
        ] {
            let expected = if lower > upper {
                (3, 0)
            } else {
                (0, value.clamp(lower, upper))
            };
            cases.push(check(
                &format!(
                    "integer.clamp({argument}, {}, {})",
                    integer(lower),
                    integer(upper)
                ),
                expected,
            ));
        }
    }
    compare(&source(&cases), true);
}

#[test]
fn monomorphic_integer_and_range_utilities_coexist_with_live_owners() {
    compare(
        "import std.int as integer; import std.range as range; import std.option as option;\n\
         fn main() -> bool {\n\
           let bytes = system_bytes_zeroed_abort(2);\n\
           let mut cursor = range.new(0, owned_bytes_len(bytes));\n\
           match range.next(inout cursor) {\n\
             option.Some(index) => integer.saturating_add(owned_bytes_get(bytes, index), 42) == 42\n\
             | option.None => false\n\
           }\n\
         }",
        false,
    );
}
