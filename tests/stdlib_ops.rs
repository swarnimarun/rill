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
            "rill-stdlib-ops-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn load(&self, source: &str) -> Result<rill::ast::Program, rill::modules::ModuleError> {
        let entry = self.0.join("main.rill");
        fs::write(&entry, source).unwrap();
        rill::modules::compile::load_program(&entry)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compare(source: &str, expected: Option<&str>, pure: bool) {
    let fixture = Fixture::new();
    let program = fixture.load(source).unwrap();
    let interpreted = rill::interpreter::run(&program).map(|value| value.to_string());
    match expected {
        Some(text) => assert_eq!(interpreted.unwrap(), text),
        None => assert_eq!(interpreted.unwrap_err(), "runtime trap"),
    }
    let c = rill::backend::emit(&program).unwrap();
    if pure {
        for allocator in ["malloc(", "calloc(", "realloc(", "rl_allocate("] {
            assert!(!c.contains(allocator), "unexpected allocator {allocator}");
        }
    }
    let path = fixture.0.join("program.c");
    fs::write(&path, c).unwrap();
    let executable = fixture.0.join("program");
    for optimization in ["-O0", "-O3"] {
        let build = Command::new("cc")
            .args(["-std=c11", optimization])
            .arg(&path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&executable).output().unwrap();
        match expected {
            Some(text) => {
                assert!(
                    run.status.success(),
                    "{}",
                    String::from_utf8_lossy(&run.stderr)
                );
                assert_eq!(run.stdout, format!("{text}\n").as_bytes());
                assert!(run.stderr.is_empty());
            }
            None => {
                assert_eq!(run.status.code(), Some(1));
                assert_eq!(run.stderr, b"rill: runtime trap\n");
                assert!(run.stdout.is_empty());
            }
        }
    }
}

#[test]
fn option_and_result_helpers_specialize_copy_callbacks_and_captures() {
    compare(
        r#"
import std.option as option;
import std.option_ops as opt;
import std.result as result;
import std.result_ops as res;
fn integer_result(value: result.Result<i32, bool>) { value }
fn result_value(value: result.Result<i32, bool>) {
    match value { result.Ok(x) => x | result.Err(_) => -1 }
}
fn identity(value) { value }
fn main() {
    let increment = 1;
    let mapper = fn(value) { value + increment };
    let a = opt.unwrap_or(opt.map(option.Some(41), mapper), 0) == 42;
    let b = opt.unwrap_or(opt.map(option.Some(true), identity), false);
    let c = opt.unwrap_or(opt.map(option.Some(42), identity), 0) == 42;
    let d = opt.unwrap_or(opt.and_then(option.Some(21), fn(x) { option.Some(x * 2) }), 0) == 42;
    let e = opt.unwrap_or(opt.or_else(option.None, fn() { option.Some(42) }), 0) == 42;
    let f = result_value(res.map(integer_result(result.Ok(21)), fn(x) { x * 2 })) == 42;
    let g = match res.map(integer_result(result.Ok(42)), fn(x) { x == 42 }) {
        result.Ok(value) => value | result.Err(_) => false
    };
    let h = match res.map_err(integer_result(result.Err(true)), fn(flag) { if flag { 7 } else { 0 } }) {
        result.Err(value) => value == 7 | result.Ok(_) => false
    };
    let i = result_value(res.and_then(integer_result(result.Ok(21)), fn(x) { integer_result(result.Ok(x * 2)) })) == 42;
    let j = result_value(res.or_else(integer_result(result.Err(true)), fn(flag) { integer_result(result.Ok(if flag { 42 } else { 0 })) })) == 42;
    let k = opt.unwrap_or(res.ok(integer_result(result.Ok(42))), 0) == 42;
    let l = opt.unwrap_or(res.err(integer_result(result.Err(true))), false);
    let m = res.is_ok(integer_result(result.Ok(42))) && res.is_err(integer_result(result.Err(false)));
    let n = opt.is_some(option.Some(42)) && opt.is_none(opt.map(option.None, fn(x) { x + 1 }));
    let o = res.unwrap_or(integer_result(result.Err(false)), 42) == 42;
    let p = res.unwrap_or_else(integer_result(result.Err(true)), fn(error) { if error { 42 } else { 0 } }) == 42;
    a && b && c && d && e && f && g && h && i && j && k && l && m && n && o && p
}
"#,
        Some("true"),
        true,
    );
}

#[test]
fn callbacks_run_only_on_the_selected_variant() {
    compare(
        r#"
import std.option as option;
import std.option_ops as opt;
import std.result as result;
import std.result_ops as res;
fn integer_result(value: result.Result<i32, bool>) { value }
fn fail() -> i32 { 1 / 0 }
fn fail_map(value: i32) -> i32 { value / 0 }
fn fail_option() -> option.Option<i32> { option.Some(fail()) }
fn fail_result(error: bool) -> result.Result<i32, bool> { result.Ok(if error { fail() } else { fail() }) }
fn main() {
    let a = opt.unwrap_or_else(option.Some(42), fail) == 42;
    let b = opt.unwrap_or(opt.or_else(option.Some(42), fail_option), 0) == 42;
    let c = opt.is_none(opt.map(option.None, fail_map));
    let d = opt.is_none(opt.and_then(option.None, fn(x) { option.Some(fail_map(x)) }));
    let e = res.unwrap_or_else(integer_result(result.Ok(42)), fn(error) { if error { fail() } else { fail() } }) == 42;
    let f = res.unwrap_or(res.or_else(integer_result(result.Ok(42)), fail_result), 0) == 42;
    let g = res.is_err(res.map(integer_result(result.Err(true)), fail_map));
    let h = res.is_err(res.and_then(integer_result(result.Err(true)), fn(x) { integer_result(result.Ok(fail_map(x))) }));
    let i = res.is_ok(res.map_err(integer_result(result.Ok(42)), fn(error) { if error { fail() } else { fail() } }));
    a && b && c && d && e && f && g && h && i
}
"#,
        Some("true"),
        true,
    );
}

#[test]
fn ordinary_fallback_arguments_remain_eager() {
    compare("import std.option as option; import std.option_ops as opt; fn main() { opt.unwrap_or(option.Some(42), 1 / 0) }", None, true);
}

#[test]
fn type_only_option_carries_owners_but_copy_helpers_reject_them() {
    let source = "import std.option as option; fn main() { let value = system_box_i32_abort(42); let wrapped = option.Some(move value); match move wrapped { option.Some(owner) => box_read(owner) | option.None => 0 } }";
    compare(source, Some("42"), false);
    for module in ["std.option_ops", "std.result_ops"] {
        let fixture = Fixture::new();
        let error = fixture
            .load(&format!("import {module}; {source}"))
            .unwrap_err();
        assert!(
            error.message.contains("OWNED") || error.message.contains("owner"),
            "{error}"
        );
    }
}

#[test]
fn bundled_types_cannot_be_shadowed_by_local_files() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("std")).unwrap();
    fs::write(fixture.0.join("std/option.rill"), "invalid source").unwrap();
    fixture.load("import std.option as option; fn main() { match option.Some(42) { option.Some(x) => x | option.None => 0 } }").unwrap();
    let graph = rill::modules::load(&fixture.0.join("main.rill")).unwrap();
    assert_eq!(
        graph.modules["std.option"].source,
        include_str!("../stdlib/option.rill")
    );
    let second = rill::modules::load(&fixture.0.join("main.rill")).unwrap();
    assert_eq!(
        graph.modules["std.option"].source,
        second.modules["std.option"].source
    );
}

#[test]
fn complete_catalogue_discovers_embedded_sources_without_import_all_execution() {
    let names = [
        "option",
        "option_ops",
        "result",
        "result_ops",
        "int",
        "ascii",
        "text",
        "bytes",
        "binary",
        "range",
        "alloc",
        "json_input",
        "json_output",
        "cli",
        "reload",
    ];
    let fixture = Fixture::new();
    let imports = names
        .iter()
        .map(|name| format!("import std.{name};\n"))
        .collect::<String>();
    fs::write(
        fixture.0.join("main.rill"),
        format!("{imports}fn main() {{ 42 }}"),
    )
    .unwrap();
    let graph = rill::modules::load(&fixture.0.join("main.rill")).unwrap();
    assert_eq!(graph.modules.len(), names.len() + 1);
    for name in names {
        let logical = format!("std.{name}");
        assert_eq!(
            graph.modules[&logical].path,
            PathBuf::from(format!("<{logical}>"))
        );
    }
    let error = fixture
        .load("import std.bytes; import std.option_ops; fn main() { 42 }")
        .unwrap_err();
    assert!(
        error.message.contains("OWNED") || error.message.contains("owner"),
        "{error}"
    );
}

#[test]
fn checked_in_utility_examples_work_with_and_without_owners() {
    compare(
        include_str!("../examples/stdlib/utilities.rill"),
        Some("42"),
        false,
    );
    compare(
        include_str!("../examples/stdlib/options.rill"),
        Some("42"),
        true,
    );
}
