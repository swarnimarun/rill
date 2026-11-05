//! Sources shipped in the compiler. No filesystem search participates here.
pub(super) fn source(name: &str) -> Option<&'static str> {
    match name {
        "std.option" => Some(include_str!("../../stdlib/option.rill")),
        "std.option_ops" => Some(include_str!("../../stdlib/option_ops.rill")),
        "std.result_ops" => Some(include_str!("../../stdlib/result_ops.rill")),
        "std.int" => Some(include_str!("../../stdlib/int.rill")),
        "std.range" => Some(include_str!("../../stdlib/range.rill")),
        "std.ascii" => Some(include_str!("../../stdlib/ascii.rill")),
        "std.text" => Some(include_str!("../../stdlib/text.rill")),
        "std.bytes" => Some(include_str!("../../stdlib/bytes.rill")),
        "std.binary" => Some(include_str!("../../stdlib/binary.rill")),
        "std.reload" => Some(include_str!("../../stdlib/reload.rill")),
        "std.json_input" => Some(include_str!("../../stdlib/json_input.rill")),
        "std.json_output" => Some(include_str!("../../stdlib/json_output.rill")),
        "std.alloc" => Some(include_str!("../../stdlib/alloc.rill")),
        "std.result" => Some(include_str!("../../stdlib/result.rill")),
        "std.cli" => Some(include_str!("../../stdlib/cli.rill")),
        _ => None,
    }
}
