use rill::lsp::{analysis, text};
use serde_json::Value;
use std::collections::BTreeMap;

fn detail(items: &[Value], name: &str) -> String {
    items
        .iter()
        .find(|item| item["label"] == name)
        .unwrap_or_else(|| panic!("missing {name}: {items:?}"))["detail"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn callable_completion_uses_inferred_types_at_lexical_bindings() {
    let source = "fn main() { let increment = fn(value) { value + 1 }; increment(41) }";
    let at = source.rfind("increment").unwrap();
    let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
    assert!(detail(&items, "increment").contains("fn(i32) -> i32"));
    let hover = analysis::hover("untitled:editor", source, at, &BTreeMap::new()).unwrap();
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("fn(i32) -> i32"),
        "{hover}"
    );
}

#[test]
fn function_values_navigate_in_whitespace_and_operator_expressions() {
    let source =
        "fn increment(value) { value + 1 } fn main() { let f = increment; 20 |> f >> increment }";
    for at in [
        source.find("= increment").unwrap() + 2,
        source.rfind("increment").unwrap(),
    ] {
        let target = analysis::definition("untitled:editor", source, at, &BTreeMap::new()).unwrap();
        assert_eq!(target["range"], text::range(source, 3..12));
    }
}

#[test]
fn unfinished_local_member_completion_uses_checked_record_identity() {
    let source = "type Point = { x: i32; ready: bool } fn main() { let p = record Point { x = 42; ready = true }; p. }";
    let at = source.rfind("p.").unwrap() + 2;
    let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
    assert_eq!(detail(&items, "x"), "x: i32");
    assert_eq!(detail(&items, "ready"), "ready: bool");
    assert!(!items.iter().any(|item| item["label"] == "Point"));
}

#[test]
fn nested_record_and_callback_members_keep_their_inferred_types() {
    let source = "type Holder<a> = { callback: a } type Outer<a> = { nested: a; count: i32 } fn main() { let h = record Outer { nested = record Holder { callback = fn(value) { value + 1 } }; count = 0 }; h.nested. }";
    let at = source.rfind("nested.").unwrap() + 7;
    let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
    assert_eq!(detail(&items, "callback"), "callback: fn(i32) -> i32");
    assert!(!items.iter().any(|item| item["label"] == "count"));
}

#[test]
fn unfinished_identifier_completion_retains_completed_lexical_bindings() {
    let source = "fn main() { let increment = fn(value) { value + 1 }; inc";
    let items = analysis::completion("untitled:editor", source, source.len(), &BTreeMap::new());
    assert!(detail(&items, "increment").contains("fn(i32) -> i32"));
}

#[test]
fn whitespace_calls_do_not_claim_later_parenthesized_signature_help() {
    let source = "fn add(left, right) { left + right } fn identity(value) { value } fn main() { let sum = add 20 22; identity(sum) }";
    let at = source.rfind("identity(").unwrap() + 9;
    let help = analysis::signature_help("untitled:editor", source, at, &BTreeMap::new()).unwrap();
    assert!(
        help["signatures"][0]["label"]
            .as_str()
            .unwrap()
            .starts_with("fn identity(value:"),
        "{help}"
    );
}

#[test]
fn imported_record_fields_follow_nominal_identity_and_unsaved_sources() {
    let directory = std::env::temp_dir().join(format!("rill-lsp-members-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let uri = text::path_to_uri(&directory.join("main.rill")).unwrap();
    let overlays = BTreeMap::from([
        (text::path_to_uri(&directory.join("left.rill")).unwrap(), "export type Point = { x: i32 } export fn make() { record Point { x = 42 } }".to_owned()),
        (text::path_to_uri(&directory.join("right.rill")).unwrap(), "export type Point = { ready: bool } export fn make() { record Point { ready = true } }".to_owned()),
    ]);
    let source = "import left; import right; fn main() { let p = right.make(); p. }";
    let items = analysis::completion(&uri, source, source.rfind("p.").unwrap() + 2, &overlays);
    assert_eq!(detail(&items, "ready"), "ready: bool");
    assert!(!items.iter().any(|item| item["label"] == "x"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn record_callback_completion_and_incomplete_signature_use_checked_members() {
    let source = "type Holder<a> = { callback: a } fn main() { let h = record Holder { callback = fn(value) { value + 1 } }; h.callback(41) }";
    let at = source.rfind("h.").unwrap() + 2;
    let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
    assert_eq!(detail(&items, "callback"), "callback: fn(i32) -> i32");
    let incomplete = source.replace("callback(41)", "callback(");
    let at = incomplete.rfind("callback(").unwrap() + 9;
    let help =
        analysis::signature_help("untitled:editor", &incomplete, at, &BTreeMap::new()).unwrap();
    assert!(
        help["signatures"][0]["label"]
            .as_str()
            .unwrap()
            .contains("i32"),
        "{help}"
    );
}

#[test]
fn shadowed_function_names_keep_local_types_and_definitions() {
    let source = "fn identity(value) { value } fn main() { let identity = fn(number) { number + 1 }; let f = identity; f(41) }";
    let at = source.rfind("identity;").unwrap();
    let target = analysis::definition("untitled:editor", source, at, &BTreeMap::new()).unwrap();
    let declared = source.find("let identity").unwrap() + 4;
    assert_eq!(target["range"], text::range(source, declared..declared + 8));
    let hover = analysis::hover("untitled:editor", source, at, &BTreeMap::new()).unwrap();
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("fn(i32) -> i32"),
        "{hover}"
    );
}

#[test]
fn polymorphic_callback_fields_remain_polymorphic_in_completion() {
    let source = "type Holder<a> = { callback: a } fn main() { let h = record Holder { callback = fn(value) { value } }; h. }";
    let at = source.rfind("h.").unwrap() + 2;
    let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
    assert_eq!(detail(&items, "callback"), "callback: fn('a) -> 'a");
}

#[test]
fn inferred_parameter_details_share_their_signature_type_variable_names() {
    for source in [
        "fn pick(x, y) { x } fn main() { 42 }",
        "fn main() { let pick = fn(x, y) { x }; 42 }",
    ] {
        let at = source.find("{ x }").unwrap() + 2;
        let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
        assert_eq!(detail(&items, "x"), "x: 'a");
        assert_eq!(detail(&items, "y"), "y: 'b");
    }
}

#[test]
fn nested_record_callback_signatures_retain_the_full_lexical_path() {
    let source = "type Holder<a> = { callback: a } type Outer<a> = { nested: a } fn main() { let h = record Outer { nested = record Holder { callback = fn(value) { value + 1 } } }; h.nested.callback(";
    let help = analysis::signature_help("untitled:editor", source, source.len(), &BTreeMap::new())
        .unwrap();
    assert_eq!(
        help["signatures"][0]["label"],
        "fn h.nested.callback(i32) -> i32"
    );
}

#[test]
fn unfinished_nested_scopes_preserve_completed_bindings_and_enclosing_braces() {
    for source in [
        "type Point = { x: i32 } fn main() { let p = record Point { x = 42 }; { p. } }",
        "type Point = { x: i32 } fn main() { let p = record Point { x = 42 }; let f = fn() { { p. } }; 42 }",
        "type Point = { x: i32 }\r\nfn main() { let text = \"😀\"; let p = record Point { x = 42 }; { p.",
    ] {
        let at = source.rfind("p.").unwrap() + 2;
        let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
        assert_eq!(detail(&items, "x"), "x: i32");
    }
    for source in [
        "fn main() { let f = fn(value) { value + 1 }; { f( } }",
        "fn main() { let f = fn(value) { value + 1 }; let g = fn() { { f( } }; 42 }",
        "fn main() { let text = \"😀\";\r\nlet f = fn(value) { value + 1 }; { f(",
    ] {
        let at = source.rfind("f(").unwrap() + 2;
        let help =
            analysis::signature_help("untitled:editor", source, at, &BTreeMap::new()).unwrap();
        assert_eq!(help["signatures"][0]["label"], "fn f(value: i32) -> i32");
    }
}

#[test]
fn unfinished_immediate_calls_use_only_the_checked_complete_callee() {
    for (source, label) in [
        (
            "fn add(left, right) { left + right } fn main() { add(20)(",
            "fn (right: i32) -> i32",
        ),
        (
            "fn make(amount) { fn(value) { amount + value } } fn main() { make(20)(",
            "fn (i32) -> i32",
        ),
        (
            "fn make() { fn() { 42 } } fn main() { make()(",
            "fn () -> i32",
        ),
        (
            "fn main() { (fn(value) { value + 1 })(",
            "fn (value: i32) -> i32",
        ),
        (
            "fn main() { let f = fn(value) { value + 1 }; { (f)( } }",
            "fn (i32) -> i32",
        ),
    ] {
        let at = source.rfind('(').unwrap() + 1;
        let help =
            analysis::signature_help("untitled:editor", source, at, &BTreeMap::new()).unwrap();
        assert_eq!(help["signatures"][0]["label"], label, "{source}");
    }
    for source in [
        "fn add(left, right) { left + right } fn main() { add(true)(",
        "fn main() { (42)(",
        "extern fn foreign(value: i32) -> i32; fn main() { (foreign)(",
        "fn main() { let f = 42; (f)(",
    ] {
        assert!(
            analysis::signature_help("untitled:editor", source, source.len(), &BTreeMap::new())
                .is_none(),
            "{source}"
        );
    }
}

#[test]
fn unfinished_binding_recovery_keeps_the_outer_scope_without_inventing_unit() {
    for (source, name, expected) in [
        (
            "fn main() { let outer = 1; let value = val }",
            "value",
            None,
        ),
        (
            "fn main() { let value = 1; let value = val }",
            "value",
            Some("let value: i32"),
        ),
        (
            "fn main() { let value = fn(x) { x + 1 }; let value = val }",
            "value",
            Some("let value: fn(i32) -> i32"),
        ),
    ] {
        let at = source.rfind("val }").unwrap() + 3;
        let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
        match expected {
            Some(expected) => assert_eq!(detail(&items, name), expected),
            None => assert!(!items.iter().any(|item| item["label"] == name), "{items:?}"),
        }
        assert!(!items.iter().any(|item| item["detail"] == "let value: unit"));
    }
    let source = "fn main() { let f = fn(value) { value + 1 }; let f = f( }";
    let at = source.rfind("f(").unwrap() + 2;
    let help = analysis::signature_help("untitled:editor", source, at, &BTreeMap::new()).unwrap();
    assert_eq!(help["signatures"][0]["label"], "fn f(value: i32) -> i32");
}

#[test]
fn parsed_unfinished_member_names_recover_only_checked_completed_bindings() {
    let source = "type Inner = { value: i32 }\ntype Outer<a> = { inner: Inner; callback: a }\nfn main() {\nlet state = record Outer { inner = record Inner { value = 41 }; callback = fn(value) { value + 1 } };\nstate.inner.va\n}\n";
    let at = source.find("inner.va").unwrap() + 8;
    let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
    assert_eq!(detail(&items, "value"), "value: i32");
}

#[test]
fn recovery_restores_the_outer_statement_boundary_after_a_completed_lambda() {
    let source = "fn main() { let outer = 42; let next = (fn(x) { x })(out }";
    let at = source.find("out }").unwrap() + 3;
    let items = analysis::completion("untitled:editor", source, at, &BTreeMap::new());
    assert_eq!(detail(&items, "outer"), "let outer: i32");
    assert!(!items.iter().any(|item| item["label"] == "next"));
}

#[test]
fn grouped_local_callees_do_not_borrow_shadowed_global_parameter_names() {
    for source in [
        "fn f(first, second) { first + second } fn main() { let f = fn(value) { value + 1 }; (f)(",
        "fn f(first, second, third) { first + second + third } fn main() { let f = fn(left, right) { left + right }; f(20)(",
    ] {
        let help = analysis::signature_help("untitled:editor", source, source.len(), &BTreeMap::new()).unwrap();
        assert_eq!(help["signatures"][0]["label"], "fn (i32) -> i32");
    }
}
