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
            "rill lsp analysis {} {}",
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

fn hovered(value: Option<Value>) -> String {
    value.unwrap()["contents"]["value"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn syntax_and_semantic_diagnostics_use_unsaved_sources_without_native_tools() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let malformed = "// 😀\nfn main() { not }";
    let diagnostics = analysis::diagnostics(&uri, malformed, &BTreeMap::new());
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "E-PARSE");
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line":1,"character":16})
    );
    let diagnostics = analysis::diagnostics(&uri, "fn main() { 1 + true }", &BTreeMap::new());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0]["message"]
        .as_str()
        .unwrap()
        .contains("E-TYPE"));
    let bindings = "@include(\"missing.h\"); @pkg_config(\"not-installed\"); export extern fn ready() -> bool;";
    assert!(analysis::diagnostics(&uri, bindings, &BTreeMap::new()).is_empty());
    assert!(analysis::diagnostics(&uri, "export fn answer() { 42 }", &BTreeMap::new()).is_empty());
}

#[test]
fn symbols_and_hover_use_compiler_types_with_utf16_ranges() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let source = "// 😀\nfn add(x) { x + 1 }\nfn main() { let number = 41; add(number) }";
    let symbols = analysis::document_symbols(source);
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0]["name"], "add");
    assert_eq!(
        symbols[0]["selectionRange"]["start"],
        json!({"line":1,"character":3})
    );
    let hover = hovered(analysis::hover(
        &uri,
        source,
        source.rfind("add").unwrap(),
        &BTreeMap::new(),
    ));
    assert!(hover.contains("fn add(x: i32) -> i32"), "{hover}");
    let hover = hovered(analysis::hover(
        &uri,
        source,
        source.rfind("number").unwrap(),
        &BTreeMap::new(),
    ));
    assert!(hover.contains("i32"), "{hover}");
    let hover = hovered(analysis::hover(
        &uri,
        source,
        source.find("number").unwrap(),
        &BTreeMap::new(),
    ));
    assert!(hover.contains("let number: i32"), "{hover}");
}

#[test]
fn local_navigation_respects_initializers_nested_shadowing_and_pattern_arms() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let source = "fn inspect(x) { let y = x; { let x = y; x }; x } fn main() { inspect(1) }";
    let first_x = source.find("(x)").unwrap() + 1;
    let inner_x = source.find("let x").unwrap() + 4;
    let inside = source.find("x };").unwrap();
    let outside = source.rfind("; x }").unwrap() + 2;
    let initializer = source.find("= x").unwrap() + 2;
    for (reference, expected) in [
        (inside, inner_x),
        (outside, first_x),
        (initializer, first_x),
    ] {
        let target = analysis::definition(&uri, source, reference, &BTreeMap::new()).unwrap();
        assert_eq!(target["range"], text::range(source, expected..expected + 1));
        assert_eq!(target["uri"], uri);
    }
    let source = "type Option<a> = None | Some(a) fn pick(x) { match x { Some(value) => value | None => 0 } } fn main() { pick(Some(42)) }";
    let target = analysis::definition(
        &uri,
        source,
        source.find("=> value").unwrap() + 3,
        &BTreeMap::new(),
    )
    .unwrap();
    let expected = source.find("Some(value)").unwrap() + 5;
    assert_eq!(target["range"], text::range(source, expected..expected + 5));
}

#[test]
fn imported_definitions_completion_and_errors_follow_open_overlays() {
    let workspace = Workspace::new();
    let main_uri = workspace.uri("main.rill");
    let helper_uri = workspace.uri("helper.rill");
    fs::write(workspace.0.join("helper.rill"), "export fn stale() { 0 }").unwrap();
    let helper = "export type Point = { x: i32 } fn secret() { 0 } export fn answer(x) { x + 1 }";
    let source = "import helper as h; fn main() { h.answer(41) }";
    let overlays = BTreeMap::from([(helper_uri.clone(), helper.to_owned())]);
    assert!(analysis::diagnostics(&main_uri, source, &overlays).is_empty());
    let target =
        analysis::definition(&main_uri, source, source.find("answer").unwrap(), &overlays).unwrap();
    assert_eq!(target["uri"], helper_uri);
    let expected = helper.find("answer").unwrap();
    assert_eq!(target["range"], text::range(helper, expected..expected + 6));
    let incomplete = "import helper as h; fn main() { h. }";
    let choices = analysis::completion(
        &main_uri,
        incomplete,
        incomplete.find("h.").unwrap() + 2,
        &overlays,
    );
    assert!(choices.iter().any(|choice| choice["label"] == "answer"));
    assert!(choices
        .iter()
        .any(|choice| choice["label"] == "Point" && choice["kind"] == 7));
    assert!(!choices
        .iter()
        .any(|choice| choice["label"] == "secret" || choice["label"] == "stale"));
    let overlays = BTreeMap::from([(
        workspace.uri("helper.rill"),
        "export fn answer(x) { x + true }".into(),
    )]);
    assert!(!analysis::diagnostics(&main_uri, source, &overlays).is_empty());
    assert_eq!(
        fs::read_to_string(workspace.0.join("helper.rill")).unwrap(),
        "export fn stale() { 0 }"
    );
}

#[test]
fn result_annotations_keep_resolved_type_navigation_and_check_body_constraints() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let helper_uri = workspace.uri("helper.rill");
    let helper = "export type Point = { x: i32 }";
    let source = "import helper as h; fn make() -> h.Point { record h.Point { x = 42 } } fn main() { let point = make(); point.x }";
    let overlays = BTreeMap::from([(helper_uri.clone(), helper.to_owned())]);
    assert!(analysis::diagnostics(&uri, source, &overlays).is_empty());
    let annotation = source.find("-> h.Point").unwrap() + "-> h.".len();
    let target = analysis::definition(&uri, source, annotation, &overlays).unwrap();
    assert_eq!(target["uri"], helper_uri);
    let expected = helper.find("Point").unwrap();
    assert_eq!(target["range"], text::range(helper, expected..expected + 5));
    let invalid = "fn answer() -> bool { 42 } fn main() { 0 }";
    assert!(!analysis::diagnostics("untitled:results", invalid, &BTreeMap::new()).is_empty());
}

#[test]
fn completion_observes_local_lifetime_and_innermost_binding() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let source = "fn main() { let outer = 1; { let inner = 2; inner }; outer }";
    let inside = analysis::completion(
        &uri,
        source,
        source.find("inner };").unwrap(),
        &BTreeMap::new(),
    );
    assert!(inside.iter().any(|choice| choice["label"] == "inner"));
    assert!(inside.iter().any(|choice| choice["label"] == "outer"));
    let outside = analysis::completion(
        &uri,
        source,
        source.rfind("outer").unwrap(),
        &BTreeMap::new(),
    );
    assert!(!outside.iter().any(|choice| choice["label"] == "inner"));
    assert!(outside.iter().any(|choice| choice["label"] == "not"));
}

#[test]
fn sdl3_source_checks_without_loading_headers_and_navigation_uses_binding_identity() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("examples/sdl3-direct/main.rill");
    let uri = text::path_to_uri(&path).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    let bindings_uri = text::path_to_uri(&root.join("examples/sdl3-direct/sdl.rill")).unwrap();
    let overlays = BTreeMap::from([(
        bindings_uri,
        include_str!("fixtures/sdl3_manual.rill").to_owned(),
    )]);
    assert!(analysis::diagnostics(&uri, &source, &overlays).is_empty());
    let target =
        analysis::definition(&uri, &source, source.find("SDL_Init").unwrap(), &overlays).unwrap();
    assert!(target["uri"]
        .as_str()
        .unwrap()
        .ends_with("/examples/sdl3-direct/sdl.rill"));
    let hover = hovered(analysis::hover(
        &uri,
        &source,
        source.find("SDL_Init").unwrap(),
        &overlays,
    ));
    assert!(hover.contains("SDL_Init(flags: u32) -> bool"), "{hover}");
}

#[test]
fn distinct_polymorphic_variables_stay_distinct_in_function_hover() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let source = "fn first(x, y) { x } fn main() { first(42, true) }";
    let hover = hovered(analysis::hover(
        &uri,
        source,
        source.find("first").unwrap(),
        &BTreeMap::new(),
    ));
    assert!(hover.contains("x: 'a, y: 'b) -> 'a"), "{hover}");
}

#[test]
fn untitled_buffers_use_the_same_library_checker_without_fabricated_modules() {
    let uri = "untitled:Untitled-1";
    let source = "fn main() { 42 }";
    assert!(analysis::diagnostics(uri, source, &BTreeMap::new()).is_empty());
    assert!(!analysis::diagnostics(uri, "fn main() { 1 + true }", &BTreeMap::new()).is_empty());
    let error = analysis::diagnostics(uri, "import helpers; fn main() { 42 }", &BTreeMap::new());
    assert_eq!(error[0]["code"], "E-LSP-URI");
    let hover = hovered(analysis::hover(
        uri,
        source,
        source.find("42").unwrap(),
        &BTreeMap::new(),
    ));
    assert!(hover.contains("i32"));
    let source = "fn id(x) { x } fn main() { id(42) }";
    let target =
        analysis::definition(uri, source, source.rfind("id").unwrap(), &BTreeMap::new()).unwrap();
    assert_eq!(target["uri"], uri);
    assert_eq!(target["range"], text::range(source, 3..5));
}

#[test]
fn whitespace_strings_and_comments_do_not_produce_hover_or_completion_noise() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let source = "fn main() { 1 + 2 } // h.";
    assert!(analysis::hover(&uri, source, source.find(" +").unwrap(), &BTreeMap::new()).is_none());
    assert!(analysis::completion(&uri, source, source.len(), &BTreeMap::new()).is_empty());
    let source = "fn main() { \"h.\" }";
    assert!(analysis::completion(
        &uri,
        source,
        source.find("h.").unwrap() + 2,
        &BTreeMap::new()
    )
    .is_empty());
}

#[test]
fn qualified_member_selection_excludes_same_named_alias_and_argument() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let helper_uri = workspace.uri("helper.rill");
    let helper = "export fn h(value) { value }";
    let source = "import helper as h; fn main() { let h = 42; h.h(h) }";
    let overlays = BTreeMap::from([(helper_uri.clone(), helper.into())]);
    assert!(analysis::diagnostics(&uri, source, &overlays).is_empty());
    let call = source.find("h.h(h)").unwrap();
    let member = analysis::definition(&uri, source, call + 2, &overlays).unwrap();
    assert_eq!(member["uri"], helper_uri);
    let declared = helper.find("h(").unwrap();
    assert_eq!(member["range"], text::range(helper, declared..declared + 1));
    let argument = analysis::definition(&uri, source, call + 4, &overlays).unwrap();
    let local = source.find("let h").unwrap() + 4;
    assert_eq!(argument["range"], text::range(source, local..local + 1));
    assert!(analysis::definition(&uri, source, call, &overlays).is_none());
    assert!(hovered(analysis::hover(&uri, source, call + 2, &overlays)).contains("fn h(value:"));
}

#[test]
fn legacy_let_completion_ends_at_its_body_instead_of_the_enclosing_block() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let source = "fn main() { let outer = (let inner = 1 in inner); outer }";
    let inner = analysis::completion(
        &uri,
        source,
        source.find("in inner").unwrap() + 3,
        &BTreeMap::new(),
    );
    assert!(inner.iter().any(|choice| choice["label"] == "inner"));
    let outer = analysis::completion(
        &uri,
        source,
        source.rfind("outer").unwrap(),
        &BTreeMap::new(),
    );
    assert!(!outer.iter().any(|choice| choice["label"] == "inner"));
}

#[test]
fn incomplete_bare_module_prefix_respects_shadowing_but_qualified_calls_do_not() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.rill");
    let overlays = BTreeMap::from([(
        workspace.uri("helper.rill"),
        "export fn action() { 42 }".into(),
    )]);
    let source = "import helper as h; fn main() { let h = 42; h. }";
    assert!(
        analysis::completion(&uri, source, source.find("h.").unwrap() + 2, &overlays).is_empty()
    );
    let source = "import helper as h; fn main() { let h = 42; h.action() }";
    assert!(analysis::diagnostics(&uri, source, &overlays).is_empty());
    let choices = analysis::completion(&uri, source, source.find("h.").unwrap() + 2, &overlays);
    assert!(choices.iter().any(|choice| choice["label"] == "action"));
}

#[test]
fn nested_open_modules_share_the_open_entry_graph_and_unsaved_dependencies() {
    let workspace = Workspace::new();
    fs::create_dir(workspace.0.join("nested")).unwrap();
    let main_uri = workspace.uri("main.rill");
    let helper_uri = workspace.uri("nested/helper.rill");
    let util_uri = workspace.uri("util.rill");
    let main = "import nested.helper as h; fn main() { h.wrapped() }";
    let helper = "import util; export fn wrapped() { util.answer() }";
    let util = "export fn answer() { 42 }";
    let mut overlays = BTreeMap::from([
        (main_uri.clone(), main.into()),
        (helper_uri.clone(), helper.into()),
        (util_uri.clone(), util.into()),
    ]);
    assert!(analysis::diagnostics(&main_uri, main, &overlays).is_empty());
    assert!(analysis::diagnostics(&helper_uri, helper, &overlays).is_empty());
    let target = analysis::definition(
        &helper_uri,
        helper,
        helper.find("answer").unwrap(),
        &overlays,
    )
    .unwrap();
    assert_eq!(target["uri"], util_uri);
    assert!(hovered(analysis::hover(
        &helper_uri,
        helper,
        helper.find("answer").unwrap(),
        &overlays
    ))
    .contains("fn answer() -> i32"));
    let incomplete = "import util; export fn wrapped() { util. }";
    let choices = analysis::completion(
        &helper_uri,
        incomplete,
        incomplete.find("util.").unwrap() + 5,
        &overlays,
    );
    assert!(choices.iter().any(|choice| choice["label"] == "answer"));
    overlays.insert(util_uri.clone(), "export fn answer() { true + 1 }".into());
    assert!(!analysis::diagnostics(&helper_uri, helper, &overlays).is_empty());
    let updated_util = "export fn next_answer() { 42 }";
    let updated_helper = "import util; export fn wrapped() { util.next_answer() }";
    overlays.insert(util_uri.clone(), updated_util.into());
    overlays.insert(helper_uri.clone(), updated_helper.into());
    assert!(analysis::diagnostics(&helper_uri, updated_helper, &overlays).is_empty());
    let target = analysis::definition(
        &helper_uri,
        updated_helper,
        updated_helper.find("next_answer").unwrap(),
        &overlays,
    )
    .unwrap();
    assert_eq!(target["uri"], util_uri);
    let start = updated_util.find("next_answer").unwrap();
    assert_eq!(
        target["range"],
        text::range(updated_util, start..start + 11)
    );
}

#[test]
fn shared_modules_with_two_open_entry_roots_do_not_guess_a_context() {
    let workspace = Workspace::new();
    fs::create_dir(workspace.0.join("nested")).unwrap();
    let main_uri = workspace.uri("main.rill");
    let nested_main_uri = workspace.uri("nested/main.rill");
    let helper_uri = workspace.uri("nested/helper.rill");
    let main = "import nested.helper as h; fn main() { h.wrapped() }";
    let nested_main = "import helper as h; fn main() { h.wrapped() }";
    let helper = "import util; export fn wrapped() { util.answer() }";
    let overlays = BTreeMap::from([
        (main_uri.clone(), main.into()),
        (nested_main_uri.clone(), nested_main.into()),
        (helper_uri.clone(), helper.into()),
        (
            workspace.uri("util.rill"),
            "export fn answer() { 42 }".into(),
        ),
        (
            workspace.uri("nested/util.rill"),
            "export fn answer() { true }".into(),
        ),
    ]);
    assert!(analysis::diagnostics(&main_uri, main, &overlays).is_empty());
    assert!(analysis::diagnostics(&nested_main_uri, nested_main, &overlays).is_empty());
    let diagnostics = analysis::diagnostics(&helper_uri, helper, &overlays);
    assert_eq!(diagnostics[0]["code"], "E-LSP-CONTEXT");
    assert!(analysis::definition(
        &helper_uri,
        helper,
        helper.find("answer").unwrap(),
        &overlays
    )
    .is_none());
    let incomplete = "import util; export fn wrapped() { util. }";
    assert!(analysis::completion(
        &helper_uri,
        incomplete,
        incomplete.find("util.").unwrap() + 5,
        &overlays
    )
    .is_empty());
}
