//! Sources shipped in the compiler. No filesystem search participates here.
pub(super) fn source(name: &str) -> Option<&'static str> {
    match name {
        "std.reload" => Some(include_str!("../../stdlib/reload.rill")),
        "std.json_input" => Some(include_str!("../../stdlib/json_input.rill")),
        "std.json_output" => Some(include_str!("../../stdlib/json_output.rill")),
        "std.alloc" => Some(include_str!("../../stdlib/alloc.rill")),
        "std.result" => Some(include_str!("../../stdlib/result.rill")),
        "std.cli" => Some(include_str!("../../stdlib/cli.rill")),
        _ => None,
    }
}
