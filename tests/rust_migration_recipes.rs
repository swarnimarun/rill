use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rill-migration-recipes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn compare(&self, source: &str, expected: &str, label: &str) {
        let entry = self.0.join("main.rill");
        fs::write(&entry, source).unwrap();
        let program = rill::modules::compile::load_program(&entry).unwrap();
        rill::check::check(&program).unwrap();
        assert_eq!(
            rill::interpreter::run(&program).unwrap().to_string(),
            expected,
            "{label}: interpreter"
        );
        let c = self.0.join("program.c");
        let executable = self.0.join("program");
        fs::write(&c, rill::backend::emit(&program).unwrap()).unwrap();
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
                "{label} {optimization}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let run = Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "{label} {optimization}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(
                run.stdout,
                format!("{expected}\n").as_bytes(),
                "{label} {optimization}"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn recipe(name: &str, expected: &str, cases: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source =
        fs::read_to_string(root.join(format!("examples/rust-migration/recipes/{name}.rill")))
            .unwrap();
    let guide = fs::read_to_string(root.join("docs/rust-migration-recipes.md")).unwrap();
    assert!(guide.contains(source.trim()), "guide drift: {name}");
    let fixture = Fixture::new();
    fixture.compare(&source, expected, name);
    assert_eq!(source.matches("fn main()").count(), 1);
    let helpers = source.replacen("fn main()", "fn recipe_demo()", 1);
    let assertions = cases
        .iter()
        .map(|case| format!("({case})"))
        .collect::<Vec<_>>()
        .join(" && ");
    fixture.compare(
        &format!("{helpers}\nfn main() {{ {assertions} }}\n"),
        "true",
        &format!("{name} edge cases"),
    );
}

#[test]
fn range_recipe_handles_empty_reversed_negative_and_exclusive_bounds() {
    recipe(
        "range-filter",
        "25",
        &[
            "sum_odd_between(3, 3) == 0",
            "sum_odd_between(5, 2) == 0",
            "sum_odd_between(0, 1) == 0",
            "sum_odd_between(1, 2) == 1",
            "sum_odd_between(-3, 4) == 0",
            "sum_odd_between(0, 8) == 16",
        ],
    );
}

#[test]
fn callback_recipe_specializes_for_different_copy_types_and_environments() {
    recipe(
        "copy-callback",
        "42",
        &[
            "apply_twice(make_offset(0), 9) == 9",
            "apply_twice(make_offset(-2), 9) == 5",
            "apply_twice(fn(value) { value * 2 }, 3) == 12",
            "apply_twice(fn(value) { !value }, true)",
        ],
    );
}

#[test]
fn count_recipe_distinguishes_classification_parse_and_policy_errors() {
    recipe(
        "count-option",
        "8",
        &[
            "match parse_count(\"--count=1\") { result.Ok(value) => value == 1 | _ => false }",
            "match parse_count(\"--count=64\") { result.Ok(value) => value == 64 | _ => false }",
            "match parse_count(\"--count=+12\") { result.Ok(value) => value == 12 | _ => false }",
            "match parse_count(\"--count=0\") { result.Err(CountOutOfRange) => true | _ => false }",
            "match parse_count(\"--count=65\") { result.Err(CountOutOfRange) => true | _ => false }",
            "match parse_count(\"--count=-1\") { result.Err(CountOutOfRange) => true | _ => false }",
            "match parse_count(\"--counts=8\") { result.Err(ExpectedCountOption) => true | _ => false }",
            "match parse_count(\"--count\") { result.Err(ExpectedCountOption) => true | _ => false }",
            "match parse_count(\"8\") { result.Err(ExpectedCountOption) => true | _ => false }",
            "match parse_count(\"--count=\") { result.Err(InvalidCount(cli.ExpectedDigit(8))) => true | _ => false }",
            "match parse_count(\"--count=12x\") { result.Err(InvalidCount(cli.InvalidDigit(10))) => true | _ => false }",
            "match parse_count(\"--count=2147483648\") { result.Err(InvalidCount(cli.IntegerOverflow(_))) => true | _ => false }",
        ],
    );
}
