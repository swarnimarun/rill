#[test]
fn library_check_shares_semantics_without_requiring_main() {
    for source in [
        "",
        "fn increment(x) { x + 1 }",
        "fn result() -> i32 { 42 }",
        "type Option<a> = None | Some(a)",
    ] {
        let program = rill::parser::parse(source).unwrap();
        assert!(rill::check::analyze_library(&program).is_ok(), "{source}");
        assert!(rill::check::analyze(&program).is_err(), "{source}");
    }
    for source in [
        "fn broken(x) { x + true }",
        "fn broken() -> bool { 42 }",
        "fn main(x) { x }",
        "fn bad() { let b = system_box_i32_abort(1); let moved = move b; box_read(b) }",
    ] {
        let program = rill::parser::parse(source).unwrap();
        assert!(rill::check::analyze_library(&program).is_err(), "{source}");
    }
}
