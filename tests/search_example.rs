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
            "rill-search-example-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn compare(&self, source: &str, expected: &str) {
        let entry = self.0.join("main.rill");
        fs::write(&entry, source).unwrap();
        let program = rill::modules::compile::load_program(&entry).unwrap();
        rill::check::check(&program).unwrap();
        assert_eq!(
            rill::interpreter::run(&program).unwrap().to_string(),
            expected
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
                "{optimization}: {}",
                String::from_utf8_lossy(&build.stderr)
            );
            let run = Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "{optimization}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(run.stdout, format!("{expected}\n").as_bytes());
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn guide() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/search-and-equality.md"))
        .unwrap()
}

#[test]
fn complete_search_guide_programs_match_native_and_interpreter() {
    let guide = guide();
    let blocks: Vec<_> = guide.split("```rill\n").skip(1).collect();
    assert_eq!(blocks.len(), 5);
    for (block, expected) in blocks.into_iter().zip(["true", "true", "true", "2", "1"]) {
        let (source, _) = block.split_once("```").unwrap();
        Fixture::new().compare(source, expected);
    }
    for name in ["first-token", "substring", "compare-values"] {
        let source = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("examples/search/{name}.rill")),
        )
        .unwrap();
        assert!(guide.contains(source.trim()), "guide drift: {name}");
    }
}

#[test]
fn token_scan_preserves_first_match_progress_and_guarded_suffix() {
    let source =
        include_str!("../examples/search/first-token.rill").replacen("fn main()", "fn demo()", 1);
    let source = format!(
        r#"{source}
fn expected(read value: Search, found: i32, visited: i32, unmatched: i32, next: i32) -> bool {{
    value.found == found && value.visited == visited &&
    value.unmatched == unmatched && value.next_index == next
}}
fn main() {{
    expected(scan(5, tokens, "zero"), 0, 1, 0, 1) &&
    expected(scan(5, tokens, "one"), 1, 2, 1, 2) &&
    expected(scan(5, tokens, "end"), 4, 5, 4, 5) &&
    expected(scan(5, tokens, "missing"), -1, 5, 5, 5) &&
    expected(scan(0, tokens, "one"), -1, 0, 0, 0) &&
    expected(scan(-1, tokens, "one"), -1, 0, 0, 0) &&
    expected(scan(5, tokens, ""), -1, 5, 5, 5) &&
    expected(scan(1, fn(index) {{ "" }}, ""), 0, 1, 0, 1)
}}
"#
    );
    Fixture::new().compare(&source, "true");
}

#[test]
fn library_find_handles_missing_empty_and_utf8_byte_offsets() {
    Fixture::new().compare(
        r#"import std.text as text;
import std.option as option;
fn offset(value: str, needle: str) -> i32 {
    match text.find(value, needle) { option.Some(index) => index | option.None => -1 }
}
fn main() {
    offset("stone", "one") == 2 &&
    offset("oneone", "one") == 0 &&
    offset("éone", "one") == 2 &&
    offset("", "") == 0 &&
    offset("one", "") == 0 &&
    offset("one", "missing") == -1
}
"#,
        "true",
    );
}
