//! Lowering tests for the three frozen syntax features: module attributes,
//! `@include` type bodies, glob uses, and keyword field paths.

use rill_ast::{
    parse_and_lower_text, BinaryOp, Expr, Frontend, Item, TypeBody, UsePath,
};

fn frontend(src: &str) -> Frontend {
    let (frontend, sm) = parse_and_lower_text("attributes.rill", src);
    assert!(
        !frontend.diagnostics.has_errors(),
        "unexpected diagnostics:\n{}",
        rill_diagnostics::render_all(&sm, &frontend.diagnostics)
    );
    frontend
}

#[test]
fn attributes_before_items() {
    let frontend = frontend("@link(\"SDL3\");\n@cfg(\"debug\");\nlet add (a: i32) = a;\n");
    let items = &frontend.module.items;
    assert_eq!(items.len(), 3);
    let Item::Attribute(link) = &items[0] else {
        panic!("first item is not an attribute: {:?}", items[0]);
    };
    assert_eq!(frontend.text(link.name), "link");
    assert_eq!(link.value.as_deref(), Some("SDL3"), "quotes are stripped");

    let Item::Attribute(cfg) = &items[1] else {
        panic!("second item is not an attribute: {:?}", items[1]);
    };
    assert_eq!(frontend.text(cfg.name), "cfg");
    assert_eq!(cfg.value.as_deref(), Some("debug"));

    assert!(matches!(items[2], Item::Fn(_)), "the item still follows");
}

#[test]
fn attribute_without_a_value() {
    let frontend = frontend("@inline;\nlet f () = ();\n");
    let Item::Attribute(attr) = &frontend.module.items[0] else {
        panic!("not an attribute");
    };
    assert_eq!(frontend.text(attr.name), "inline");
    assert_eq!(attr.value, None);
}

#[test]
fn include_type_body() {
    let frontend = frontend("type M = @include(\"SDL3/SDL.h\");\n");
    let Item::Type(decl) = &frontend.module.items[0] else {
        panic!("not a type declaration");
    };
    match &decl.body {
        TypeBody::Include { path, .. } => assert_eq!(path, "SDL3/SDL.h"),
        other => panic!("expected an include body, got {other:?}"),
    }
}

#[test]
fn plain_and_glob_uses() {
    let frontend = frontend("use M;\nuse A::B::*;\n");
    let Item::Use(plain) = &frontend.module.items[0] else {
        panic!("not a use declaration");
    };
    let UsePath::Plain(segments) = &plain.path else {
        panic!("expected a plain path, got {:?}", plain.path);
    };
    let names: Vec<&str> = segments.iter().map(|s| frontend.text(*s)).collect();
    assert_eq!(names, vec!["M"]);

    let Item::Use(glob) = &frontend.module.items[1] else {
        panic!("not a use declaration");
    };
    let UsePath::Glob { prefix } = &glob.path else {
        panic!("expected a glob path, got {:?}", glob.path);
    };
    let names: Vec<&str> = prefix.iter().map(|s| frontend.text(*s)).collect();
    assert_eq!(names, vec!["A", "B"]);
}

#[test]
fn keyword_field_names_lower_to_path_segments() {
    let frontend = frontend(
        "let f (ev: Event) = ev.type;\n\
         let g (x: X) = x.view;\n\
         let h (p: P) = p.match;\n",
    );
    for (index, segment) in [(0usize, "type"), (1, "view"), (2, "match")] {
        let Item::Fn(decl) = &frontend.module.items[index] else {
            panic!("not a function");
        };
        let body = decl.body.as_ref().expect("a body");
        let Expr::Binary { op, rhs, .. } = body else {
            panic!("expected a flow expression, got {body:?}");
        };
        assert_eq!(*op, BinaryOp::Flow);
        let Expr::Path { segments, .. } = rhs.as_ref() else {
            panic!("expected a path on the right of `.`, got {rhs:?}");
        };
        assert_eq!(frontend.text(segments[0]), segment);
    }
}
