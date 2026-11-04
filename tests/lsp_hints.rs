use rill::lsp::{analysis, text};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rill hints {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn uri(&self, name: &str) -> String {
        text::path_to_uri(&self.0.join(name)).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn hints(source: &str) -> Vec<Value> {
    analysis::inlay_hints("untitled:hints", source, 0..source.len(), &BTreeMap::new())
}
fn has_hint(hints: &[Value], source: &str, byte: usize, label: &str, kind: u32) -> bool {
    hints.iter().any(|h| {
        h["position"] == text::position(source, byte) && h["label"] == label && h["kind"] == kind
    })
}
fn signature(source: &str, needle: &str) -> Value {
    analysis::signature_help(
        "untitled:hints",
        source,
        source.rfind(needle).unwrap() + needle.len(),
        &BTreeMap::new(),
    )
    .unwrap()
}

#[test]
fn inferred_hints_use_checker_types_and_omit_explicit_parameters_and_named_arguments() {
    let source =
        "fn add(left: i32, right) { left + right } fn main() { let right = 2; add(40, right) }";
    let result = hints(source);
    assert!(!has_hint(
        &result,
        source,
        source.find("left:").unwrap() + 4,
        ": i32",
        1
    ));
    assert!(
        has_hint(
            &result,
            source,
            source.find("right)").unwrap() + 5,
            ": i32",
            1
        ),
        "{result:?}"
    );
    assert!(has_hint(
        &result,
        source,
        source.find(") {").unwrap() + 1,
        " -> i32",
        1
    ));
    assert!(has_hint(
        &result,
        source,
        source.find("right =").unwrap() + 5,
        ": i32",
        1
    ));
    assert!(has_hint(
        &result,
        source,
        source.find("40").unwrap(),
        "left:",
        2
    ));
    assert!(!result.iter().any(|h| h["label"] == "right:"));
    assert!(hints("fn main() { let broken = missing; broken }").is_empty());
}

#[test]
fn explicit_function_results_keep_parameter_hints_without_repeating_result_types() {
    let source = "fn inferred(value) { value + 1 }\nfn annotated(value) -> i32 { value + 1 }\nfn main() -> i32 { annotated(inferred(40)) }";
    let result = hints(source);
    assert!(has_hint(
        &result,
        source,
        source.find(") {").unwrap() + 1,
        " -> i32",
        1
    ));
    let parameter_end = source.find("annotated(value)").unwrap() + "annotated(value".len();
    assert!(has_hint(&result, source, parameter_end, ": i32", 1));
    for declaration in ["annotated(value)", "main()"] {
        let end = source.find(declaration).unwrap() + declaration.len();
        assert!(
            !result
                .iter()
                .any(|hint| hint["position"] == text::position(source, end)),
            "explicit result annotation has a redundant hint: {result:?}"
        );
    }
    assert!(result.iter().any(|hint| hint["label"] == "value:"));
    let signature = signature(source, "annotated(");
    assert!(signature["signatures"][0]["label"]
        .as_str()
        .unwrap()
        .contains("annotated(value: i32) -> i32"));
    let source = "type Option<a> = None | Some(a)\nfn absent() -> Option<i32> { None }\nfn main() { absent() }";
    let result = hints(source);
    let end = source.find("absent()").unwrap() + "absent()".len();
    assert!(!result
        .iter()
        .any(|hint| hint["position"] == text::position(source, end)));
    assert!(has_hint(
        &result,
        source,
        source.find("main()").unwrap() + "main()".len(),
        " -> Option<i32>",
        1
    ));
}

#[test]
fn inlay_ranges_and_positions_are_utf16_and_hints_are_sorted() {
    let source = "fn main() { let text = \"😀\"; let count = byte_len(text); count }";
    let start = source.find("count =").unwrap() + 5;
    let result =
        analysis::inlay_hints("untitled:hints", source, start..start + 1, &BTreeMap::new());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["position"], text::position(source, start));
    assert_eq!(result[0]["label"], ": i32");
    assert_ne!(text::position(source, start)["character"], json!(start));
    assert!(
        analysis::inlay_hints("untitled:hints", source, start..start, &BTreeMap::new()).is_empty()
    );
}

#[test]
fn signature_help_tracks_nested_arguments_and_ignores_string_commas() {
    let source = "fn pair(left, right) { left + right } fn main() { pair(pair(1, 2), 39) }";
    let inner = signature(source, "pair(1,");
    assert_eq!(inner["activeParameter"], 1);
    assert!(inner["signatures"][0]["label"]
        .as_str()
        .unwrap()
        .contains("left: i32, right: i32"));
    let outer = signature(source, "pair(pair(1, 2),");
    assert_eq!(outer["activeParameter"], 1);
    let source =
        "fn pick(text, number) { byte_len(text) + number } fn main() { pick(\"a,(b)\", 1) }";
    assert_eq!(signature(source, "pick(\"a,(b)\",")["activeParameter"], 1);
    assert!(analysis::signature_help(
        "untitled:hints",
        source,
        source.find("a,(").unwrap() + 2,
        &BTreeMap::new()
    )
    .is_none());
}

#[test]
fn native_and_constructor_signature_help_uses_real_declarations() {
    let source = "extern fn ready(flags: u32) -> bool; fn main() { ready(u32(1)) }";
    let result = signature(source, "ready(");
    assert_eq!(
        result["signatures"][0]["label"],
        "extern fn ready(flags: u32) -> bool"
    );
    let source = "type Option<a> = None | Some(a) fn main() { Some(42) }";
    let result = signature(source, "Some(");
    assert_eq!(
        result["signatures"][0]["parameters"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(result["signatures"][0]["label"]
        .as_str()
        .unwrap()
        .contains("Option<a>"));
}

#[test]
fn incomplete_calls_keep_import_overlays_and_do_not_borrow_shadowed_signatures() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let helper = workspace.uri("helper.rill");
    let overlays = BTreeMap::from([(
        helper,
        "export fn answer(first, second) { first + second }".to_owned(),
    )]);
    let source = "import helper as h; fn main() { h.answer(1, }";
    let result =
        analysis::signature_help(&uri, source, source.find("1,").unwrap() + 2, &overlays).unwrap();
    assert!(result["signatures"][0]["label"]
        .as_str()
        .unwrap()
        .contains("first: i32, second: i32"));
    assert_eq!(result["activeParameter"], 1);
    let source = "fn add(first, second) { first + second } fn main() { add(1,";
    let result =
        analysis::signature_help("untitled:hints", source, source.len(), &BTreeMap::new()).unwrap();
    assert_eq!(result["activeParameter"], 1);
    let source =
        "fn add(first, second) { first + second } fn main() { let answer = add()(20,22); answer }";
    let at = source.find("add()(").unwrap() + 4;
    let result = analysis::signature_help("untitled:hints", source, at, &BTreeMap::new()).unwrap();
    assert_eq!(result["activeParameter"], 0);
    let source = "fn add(first, second) { first + second } fn main() { let add = 1; add( }";
    assert!(analysis::signature_help(
        "untitled:hints",
        source,
        source.rfind("add(").unwrap() + 4,
        &BTreeMap::new()
    )
    .is_none());
    let source = "fn main() { let add = fn(first, second) { first + second }; add(1,";
    let result =
        analysis::signature_help("untitled:hints", source, source.len(), &BTreeMap::new()).unwrap();
    assert_eq!(result["activeParameter"], 1);
    assert!(result["signatures"][0]["label"]
        .as_str()
        .unwrap()
        .contains("first: i32, second: i32"));
}

#[test]
fn lambda_scopes_callable_hints_and_local_signatures_use_checked_function_types() {
    let source = "fn main() { let f = fn(value) { value + 1 }; f(41) }";
    let result = hints(source);
    assert!(
        has_hint(
            &result,
            source,
            source.find("value)").unwrap() + 5,
            ": i32",
            1
        ),
        "{result:?}"
    );
    assert!(
        result.iter().any(|h| h["label"]
            .as_str()
            .is_some_and(|label| label.contains("fn(i32) -> i32"))),
        "{result:?}"
    );
    assert!(
        has_hint(&result, source, source.find("41").unwrap(), "value:", 2),
        "{result:?}"
    );
    assert!(!result
        .iter()
        .any(|h| h["label"].as_str().unwrap().contains("$lambda")));
    let help = signature(source, "f(");
    assert_eq!(help["signatures"][0]["label"], "fn f(value: i32) -> i32");
    let reference = source.find("value +").unwrap();
    let target =
        analysis::definition("untitled:hints", source, reference, &BTreeMap::new()).unwrap();
    assert_eq!(
        target["range"],
        text::range(
            source,
            source.find("value)").unwrap()..source.find("value)").unwrap() + 5
        )
    );
    let source = "fn main() { (fn(value) { value + 1 })(41) }";
    assert!(signature(source, ")(41")["signatures"][0]["label"]
        .as_str()
        .unwrap()
        .contains("i32"));
}

#[test]
fn partial_composed_and_polymorphic_local_functions_keep_their_checked_call_types() {
    let source =
        "fn add(left, right) { left + right } fn main() { let increment = add(1); increment(41) }";
    let result = hints(source);
    assert!(has_hint(
        &result,
        source,
        source.find("41").unwrap(),
        "right:",
        2
    ));
    assert_eq!(
        signature(source, "increment(")["signatures"][0]["label"],
        "fn increment(right: i32) -> i32"
    );
    let source = "fn main() { let increment = fn(number) { number + 1 }; let twice = increment >> increment; twice(40) }";
    let help = signature(source, "twice(");
    assert_eq!(help["signatures"][0]["label"], "fn twice(i32) -> i32");
    let source = "fn identity(value) { value } fn main() { let f = identity; let number = f(42); if f(true) { number } else { 0 } }";
    assert_eq!(
        signature(source, "f(42")["signatures"][0]["label"],
        "fn f(value: i32) -> i32"
    );
    assert_eq!(
        signature(source, "f(true")["signatures"][0]["label"],
        "fn f(value: bool) -> bool"
    );
}

#[test]
fn imported_module_hints_and_incomplete_signatures_retain_the_open_entry_root() {
    let workspace = Workspace::new();
    fs::create_dir(workspace.0.join("nested")).unwrap();
    let main_uri = workspace.uri("main.rill");
    let helper_uri = workspace.uri("nested/helper.rill");
    let util_uri = workspace.uri("util.rill");
    let main = "import nested.helper as h; fn main() { h.wrapped() }";
    let helper = "import util; export fn wrapped() { util.answer(41) }";
    let overlays = BTreeMap::from([
        (main_uri, main.to_owned()),
        (helper_uri.clone(), helper.to_owned()),
        (util_uri, "export fn answer(value) { value + 1 }".to_owned()),
    ]);
    let result = analysis::inlay_hints(&helper_uri, helper, 0..helper.len(), &overlays);
    assert!(has_hint(
        &result,
        helper,
        helper.find("41").unwrap(),
        "value:",
        2
    ));
    let incomplete = "import util; export fn wrapped() { util.answer( }";
    let help = analysis::signature_help(
        &helper_uri,
        incomplete,
        incomplete.find("answer(").unwrap() + 7,
        &overlays,
    )
    .unwrap();
    assert_eq!(
        help["signatures"][0]["label"],
        "fn answer(value: i32) -> i32"
    );
}

#[test]
fn lambda_hints_share_type_variables_between_parameters_and_result() {
    let source = "fn main() { let f = fn(x, y) { y }; 42 }";
    let result = hints(source);
    assert!(has_hint(
        &result,
        source,
        source.find("x,").unwrap() + 1,
        ": 'a",
        1
    ));
    assert!(has_hint(
        &result,
        source,
        source.find("y)").unwrap() + 1,
        ": 'b",
        1
    ));
    assert!(
        has_hint(&result, source, source.find("y)").unwrap() + 2, " -> 'b", 1),
        "{result:?}"
    );
}

#[test]
fn signature_help_preserves_returned_function_layers() {
    let source = "fn main() { let f = fn(x) { fn() { x } }; f(42)() }";
    assert_eq!(
        signature(source, "f(42")["signatures"][0]["label"],
        "fn f(x: i32) -> fn() -> i32"
    );
    let source = "fn main() { let f = fn(x) { fn(y) { x + y } }; f(20)(22) }";
    assert_eq!(
        signature(source, "f(20")["signatures"][0]["label"],
        "fn f(x: i32) -> fn(i32) -> i32"
    );
}
